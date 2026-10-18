pub mod bucket;
pub mod bucket_sync;
pub mod credential;
pub mod credential_bucket;
pub mod credential_sync;
pub mod organization;
pub mod organization_member;

fn new_sync_id() -> Result<uuid::Uuid, sea_orm::DbErr> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|error| {
        sea_orm::DbErr::Custom(format!("cannot generate synchronization UUID: {error}"))
    })?;
    Ok(uuid::Builder::from_random_bytes(bytes).into_uuid())
}
