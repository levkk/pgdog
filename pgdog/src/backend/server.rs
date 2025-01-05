//! PostgreSQL serer connection.
use std::time::{Duration, Instant};

use bytes::{BufMut, BytesMut};
use rustls_pki_types::ServerName;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    spawn,
};
use tracing::{debug, info};

use super::{pool::Address, Error};
use crate::net::{
    messages::{hello::SslReply, FromBytes, Protocol, Startup, ToBytes},
    parameter::Parameters,
    tls::connector,
    Parameter, Stream,
};
use crate::state::State;
use crate::{
    net::messages::{
        Authentication, BackendKeyData, ErrorResponse, Message, ParameterStatus, Query,
        ReadyForQuery, Terminate,
    },
    stats::ConnStats,
};

/// PostgreSQL server connection.
pub struct Server {
    addr: Address,
    stream: Option<Stream>,
    id: BackendKeyData,
    params: Parameters,
    state: State,
    created_at: Instant,
    last_used_at: Instant,
    last_healthcheck: Option<Instant>,
    stats: ConnStats,
}

impl Server {
    /// Create new PostgreSQL server connection.
    pub async fn connect(addr: &Address) -> Result<Self, Error> {
        debug!("=> {}", addr);
        let mut stream = Stream::plain(TcpStream::connect(addr.to_string()).await?);

        // Request TLS.
        stream.write_all(&Startup::tls().to_bytes()?).await?;
        stream.flush().await?;

        let mut ssl = BytesMut::new();
        ssl.put_u8(stream.read_u8().await?);
        let ssl = SslReply::from_bytes(ssl.freeze())?;

        if ssl == SslReply::Yes {
            let connector = connector()?;
            let plain = stream.take()?;

            let server_name = ServerName::try_from(addr.host.clone())?;

            let cipher =
                tokio_rustls::TlsStream::Client(connector.connect(server_name, plain).await?);

            stream = Stream::tls(cipher);
        }

        stream.write_all(&Startup::new().to_bytes()?).await?;
        stream.flush().await?;

        // Perform authentication.
        loop {
            let message = stream.read().await?;

            match message.code() {
                'E' => {
                    let error = ErrorResponse::from_bytes(message.payload())?;
                    return Err(Error::ConnectionError(error));
                }
                'R' => {
                    let auth = Authentication::from_bytes(message.payload())?;

                    match auth {
                        Authentication::Ok => break,
                    }
                }

                code => return Err(Error::UnexpectedMessage(code)),
            }
        }

        let mut params = Parameters::default();
        let mut key_data: Option<BackendKeyData> = None;

        loop {
            let message = stream.read().await?;

            match message.code() {
                // ReadyForQery (B)
                'Z' => break,
                // ParameterStatus (B)
                'S' => {
                    let parameter = ParameterStatus::from_bytes(message.payload())?;
                    params.push(Parameter {
                        name: parameter.name,
                        value: parameter.value,
                    });
                }
                // BackendKeyData (B)
                'K' => {
                    key_data = Some(BackendKeyData::from_bytes(message.payload())?);
                }

                code => return Err(Error::UnexpectedMessage(code)),
            }
        }

        let id = key_data.ok_or(Error::NoBackendKeyData)?;

        info!("new server connection [{}]", addr);

        Ok(Server {
            addr: addr.clone(),
            stream: Some(stream),
            id,
            params,
            state: State::Idle,
            created_at: Instant::now(),
            last_used_at: Instant::now(),
            last_healthcheck: None,
            stats: ConnStats::default(),
        })
    }

    /// Request query cancellation for the given backend server identifier.
    pub async fn cancel(addr: &str, id: &BackendKeyData) -> Result<(), Error> {
        let mut stream = TcpStream::connect(addr).await?;
        stream
            .write_all(
                &Startup::Cancel {
                    pid: id.pid,
                    secret: id.secret,
                }
                .to_bytes()?,
            )
            .await?;
        stream.flush().await?;

        Ok(())
    }

    /// Send messages to the server.
    pub async fn send(&mut self, messages: Vec<impl Protocol>) -> Result<(), Error> {
        self.state = State::Active;
        match self.stream().send_many(messages).await {
            Ok(sent) => {
                self.stats.bytes_sent += sent;
                Ok(())
            }
            Err(err) => {
                self.state = State::Error;
                Err(err.into())
            }
        }
    }

    /// Flush all pending messages making sure they are sent to the server immediately.
    pub async fn flush(&mut self) -> Result<(), Error> {
        if let Err(err) = self.stream().flush().await {
            self.state = State::Error;
            Err(err.into())
        } else {
            Ok(())
        }
    }

    /// Read a single message from the server.
    pub async fn read(&mut self) -> Result<Message, Error> {
        let message = match self.stream().read().await {
            Ok(message) => message,
            Err(err) => {
                self.state = State::Error;
                return Err(err.into());
            }
        };

        self.stats.bytes_received += message.len();

        if message.code() == 'Z' {
            self.stats.queries += 1;

            let rfq = ReadyForQuery::from_bytes(message.payload())?;

            match rfq.status {
                'I' => {
                    self.state = State::Idle;
                    self.stats.transactions += 1;
                    self.last_used_at = Instant::now();
                }
                'T' => self.state = State::IdleInTransaction,
                'E' => self.state = State::TransactionError,
                status => {
                    self.state = State::Error;
                    return Err(Error::UnexpectedTransactionStatus(status));
                }
            }
        }

        Ok(message)
    }

    /// Server sent everything.
    #[inline]
    pub fn done(&self) -> bool {
        self.state == State::Idle
    }

    /// Server connection is synchronized and can receive more messages.
    #[inline]
    pub fn in_sync(&self) -> bool {
        matches!(
            self.state,
            State::IdleInTransaction | State::TransactionError | State::Idle
        )
    }

    /// Server is still inside a transaction.
    #[inline]
    pub fn in_transaction(&self) -> bool {
        matches!(
            self.state,
            State::IdleInTransaction | State::TransactionError
        )
    }

    /// The server connection permanently failed.
    #[inline]
    pub fn error(&self) -> bool {
        self.state == State::Error
    }

    /// Server parameters.
    #[inline]
    pub fn params(&self) -> &Parameters {
        &self.params
    }

    /// Execute a query on the server and return the result.
    pub async fn execute(&mut self, query: &str) -> Result<Vec<Message>, Error> {
        if !self.in_sync() {
            return Err(Error::NotInSync);
        }

        self.send(vec![Query::new(query)]).await?;

        let mut messages = vec![];

        while !self.in_sync() {
            messages.push(self.read().await?);
        }

        Ok(messages)
    }

    /// Perform a healthcheck on this connection using the provided query.
    pub async fn healthcheck(&mut self, query: &str) -> Result<(), Error> {
        debug!("running healthcheck \"{}\" [{}]", query, self.addr);

        self.execute(query).await?;
        self.last_healthcheck = Some(Instant::now());

        Ok(())
    }

    /// Attempt to rollback the transaction on this server, if any has been started.
    pub async fn rollback(&mut self) {
        if self.in_transaction() {
            if let Err(_err) = self.execute("ROLLBACK").await {
                self.state = State::Error;
            }
        }

        if !self.in_sync() {
            self.state = State::Error;
        }
    }

    /// Server connection unique identifier.
    #[inline]
    pub fn id(&self) -> &BackendKeyData {
        &self.id
    }

    /// How old this connection is.
    #[inline]
    pub fn age(&self, instant: Instant) -> Duration {
        instant.duration_since(self.created_at)
    }

    /// How long this connection has been idle.
    #[inline]
    pub fn idle_for(&self, instant: Instant) -> Duration {
        instant.duration_since(self.last_used_at)
    }

    /// How long has it been since the last connection healthcheck.
    #[inline]
    pub fn healthcheck_age(&self, instant: Instant) -> Duration {
        if let Some(last_healthcheck) = self.last_healthcheck {
            instant.duration_since(last_healthcheck)
        } else {
            Duration::MAX
        }
    }

    /// Get server address.
    #[inline]
    pub fn addr(&self) -> &Address {
        &self.addr
    }

    #[inline]
    fn stream(&mut self) -> &mut Stream {
        self.stream.as_mut().unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let mut stream = self.stream.take().unwrap();

        info!("closing server connection [{}]", self.addr,);

        spawn(async move {
            stream.write_all(&Terminate.to_bytes()?).await?;
            stream.flush().await?;
            Ok::<(), Error>(())
        });
    }
}