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
    ],
    search: { provider: 'local' },
  },
})
