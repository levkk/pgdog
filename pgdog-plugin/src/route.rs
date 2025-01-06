use crate::bindings::*;

impl Route {
    ///
    pub fn unknown() -> Route {
        Route {
            shard: Shard_ANY,
            affinity: Affinity_UNKNOWN,
        }
    }

    /// Is this a read?
    pub fn read(&self) -> bool {
        self.affinity == Affinity_READ
    }

    /// Is this a write?
    pub fn write(&self) -> bool {
        self.affinity == Affinity_WRITE
    }

    /// Which shard, if any.
    pub fn shard(&self) -> Option<usize> {
        if self.shard < 0 {
            None
        } else {
            Some(self.shard as usize)
        }
    }

    pub fn any_shard(&self) -> bool {
        self.shard == Shard_ANY
    }

    /// Query should go across all shards.
    pub fn cross_shard(&self) -> bool {
        self.shard == Shard_ALL
    }
}