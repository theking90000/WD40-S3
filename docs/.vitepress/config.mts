import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'WD40-S3',
  description: 'Documentation for Discord-backed object storage written in Rust.',
  lang: 'en-US',
  themeConfig: {
    nav: [
      { text: 'Documentation', link: '/' },
    ],
    sidebar: [
      { text: 'Introduction', link: '/' },
      {
        text: 'Guides',
        items: [
          { text: 'Self Host', link: '/guides/self-host' },
          { text: 'Create a webhook', link: '/guides/create-webhook' },
          { text: 'Create a bot token', link: '/guides/create-bot-token' },
          { text: 'Create a Google OIDC Client', link: '/guides/create-google-oidc-client'}
        ],
      },
    ],
    search: { provider: 'local' },
  },
})
