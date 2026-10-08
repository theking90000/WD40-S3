use backends::RegistryBackend;
use sled::Db;

#[derive(Clone)]
pub struct LocalFileRegistry {
    database: Db,
    registry: RegistryBackend,
}
