pub trait FileBackend {}

pub enum Backend {
    #[cfg(feature = "discord")]
    DiscordBackend,
    #[cfg(feature = "telegram")]
    Telegram,
}
