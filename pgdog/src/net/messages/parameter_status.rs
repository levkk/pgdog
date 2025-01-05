//! ParameterStatus (B) message.

use crate::net::{
    c_string_buf,
    messages::{code, prelude::*},
    Parameter,
};

/// ParameterStatus (B) message.
pub struct ParameterStatus {
    /// Parameter name, e.g. `client_encoding`.
    pub name: String,
    /// Parameter value, e.g. `UTF8`.
    pub value: String,
}

impl From<Parameter> for ParameterStatus {
    fn from(value: Parameter) -> Self {
        ParameterStatus {
            name: value.name,
            value: value.value,
        }
    }
}

impl ParameterStatus {
    /// Fake parameter status messages we can return
    /// to a client to make this seem like a legitimate PostgreSQL connection.
    pub fn fake() -> Vec<ParameterStatus> {
        vec![
            ParameterStatus {
                name: "server_name".into(),
                value: "pgDog".into(),
            },
            ParameterStatus {
                name: "server_encoding".into(),
                value: "UTF8".into(),
            },
            ParameterStatus {
                name: "client_encoding".into(),
                value: "UTF8".into(),
            },
        ]
    }
}

impl ToBytes for ParameterStatus {
    fn to_bytes(&self) -> Result<bytes::Bytes, crate::net::Error> {
        let mut payload = Payload::named(self.code());

        payload.put_string(&self.name);
        payload.put_string(&self.value);

        Ok(payload.freeze())
    }
}

impl FromBytes for ParameterStatus {
    fn from_bytes(mut bytes: Bytes) -> Result<Self, Error> {
        code!(bytes, 'S');

        let _len = bytes.get_i32();

        let name = c_string_buf(&mut bytes);
        let value = c_string_buf(&mut bytes);

        Ok(Self { name, value })
    }
}

impl Protocol for ParameterStatus {
    fn code(&self) -> char {
        'S'
    }
}