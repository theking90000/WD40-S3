# WD40

<img src=".github/assets/banner.png"
     alt="WD40-S3 - Discord-backed object storage"
     width="100%">

<!-- Store your files on Discord using object storage APIs. -->

WD40-S3 is an object storage system built on unconventional storage backends (aka Discord).

It is designed around three principles:

- High performance and throughput, even with unconventional backends.
- Composable systems that are easy to understand.
- API interoperability, with S3 support for integration with existing tools and systems.

## Store files on Discord

Storing files on Discord isn't really complicated. However, building reliable storage that integrates with existing systems isn't straightforward.

The main limitations are:

- **File size:** currently limited to 20 MB per file, so storing larger files isn't trivial.
- **Data privacy:** Discord (in which Tencent holds shares), Cloudflare (the spying proxy), and Google Cloud can access your data.
- **Reliability:** managing bots and webhooks, rate limits, API changes and restrictions, the expiration of some URLs, and caching mechanisms.
- **Interoperability:** the APIs are proprietary and specific to Discord, requiring custom integrations with existing systems.

WD40-S3 addresses these limitations:

- **File size:** larger uploads are split across multiple files and uploaded concurrently.
- **Data privacy:** on-the-fly block encryption allows random access.
- **Reliability:** centralized management handles credentials, rate limits, API constraints, and URL expiration.
- **Interoperability:** a complete storage system is exposed through unified APIs, including S3 support.

## Project Origin

This project is part of a series of Discord storage projects:

- [theking90000/discord-s3](https://github.com/theking90000/discord-s3) (2021): the first attempt at a rudimentary storage server, written in TypeScript, with AES-256 encryption and a React web interface.
- [UwUDev/discord-us](https://github.com/UwUDev/discord-us) (2023): a Rust system with multithreaded uploads, fast seeking, AES-256 encryption, and custom APIs that weren't designed for interoperability. Killed by Discord's introduction of signed URLs.
- discord-uss / wd40 (2024, private): Rust and Java, a master/worker architecture, a proprietary file system, and signed URL support on top of discord-us.
- [theking90000/rust-storage-streamer](https://github.com/theking90000/rust-storage-streamer) (2026): an async rewrite of discord-us in Rust, with download improvements and S3 support.
