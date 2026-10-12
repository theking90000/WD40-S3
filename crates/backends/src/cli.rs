//! Discord options composable with `#[command(flatten)]`.

use tokio_tcp_pool::Pool;

use crate::{DiscordFile, DiscordRegistry, FileBackend, RegistryBackend};

/// Discord credentials used with caller-owned connection pools.
#[derive(clap::Args)]
pub struct DiscordBackendConfig {
    /// Discord webhook URL, required for upload. Repeat or use comma-separated values.
    #[arg(
        long = "webhook",
        global = true,
        env = "WD40_DISCORD_WEBHOOK_URLS",
        value_delimiter = ',',
        hide_env_values = true
    )]
    pub webhooks: Vec<String>,

    /// Optional bot token for URL renewal. Repeat the option or use comma-separated values.
    #[arg(
        long = "bot-token",
        global = true,
        env = "WD40_DISCORD_BOT_TOKENS",
        value_delimiter = ',',
        hide_env_values = true
    )]
    pub bot_tokens: Vec<String>,
}

impl DiscordBackendConfig {
    /// Uses caller-owned TLS pools targeting `cdn.discordapp.com:443` for reads
    /// and `discord.com:443` for uploads and URL renewal.
    pub fn build(self, read_pool: Pool, write_pool: Pool) -> (FileBackend, RegistryBackend) {
        (
            FileBackend::Discord(DiscordFile::new(read_pool, write_pool.clone())),
            RegistryBackend::Discord(DiscordRegistry::new(
                write_pool,
                self.webhooks,
                self.bot_tokens,
            )),
        )
    }
}
