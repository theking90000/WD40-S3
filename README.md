<!-- # WD40 -->

<!--
<img src=".github/assets/banner.webp"
     alt="WD40-S3 - Discord-backed object storage"
     width="100%">
-->

## ⚡ Turn Discord into high-performance S3 object storage

Ever thought about storing your data on Discord? People have tried it before. With WD40-S3, the goal is to make it fast, keep memory usage low, and let you use the storage tools you already have.

WD40-S3 is an object storage system written in Rust, with Discord as its storage backend and S3 support for existing applications. Want to use it for backups? Point your S3 client at it. Want to mount it as a drive? Use a tool like rclone. Prefer a web interface? It comes with a console, and you can build your own on top of the API.

It has reached **350 MB/s (2.8 Gbps)** download speed on a cheap **2-vCPU VPS**, using **80 MB of RAM** in total.

Under the hood, it uses async Rust, parallel uploads, URL prefetching and pipelining. Large files are split into chunks, with on-the-fly block encryption that still allows random access.

You also get:

- S3 support tested with the AWS CLI, rclone and JuiceFS. Full S3 API coverage is still a work in progress.
- A single executable with embedded components, or separate components you can run independently.
- An integrated cloud console where users manage their buckets, access keys and usage.
- Optional high-resolution metrics collection.

## 🚀 Try it or self-host

Don't want to host it yourself? Try the demo at [console.6700067.xyz](https://console.6700067.xyz). Create an account, make a bucket, and connect your S3 client.

It's a demo, with no guarantees. Files are encrypted before they reach Discord, but we hold the keys needed to decrypt and serve them. Use non-sensitive data, or encrypt it yourself before uploading.

For self-hosting, you'll need Discord webhooks and a bot token for faster CDN URL refreshing. Using Discord as storage is at your own risk.

See the [portal setup](portal/README.md) to run the console, or the [file registry documentation](crates/file-registry/README.md) for the Discord storage configuration. You can also use WD40-S3 without S3, since the adapter is optional.

## 🔧 Dealing with Discord

Building a nice web interface to upload files to Discord is easy. Building a fast, reliable storage system behind it is another story.

- 🦀 The storage engine, metadata services and HTTP/S3 adapters are all **written in Rust**.

- 🧠 **Uploads and downloads run as streams**, without loading entire files into RAM.

  Buffers are reused and exchanged between stages to avoid unnecessary copies, with configurable shared budgets for pipeline buffers.

- ⚡ **While one file waits on the network, others can keep progressing.**

  Concurrent file operations share a single async task, coordinated by hand-written state machines to keep scheduling overhead low.

- 🎯 **Need to jump into the middle of a large file?**

  Byte-range reads are built into the storage engine and exposed over HTTP with `206 Partial Content` and `Content-Range`. It reads the frames covering your range, even with encryption enabled, without downloading the whole object first.

- 🗃️ **Metadata lives in persistent registries**, with transactional object updates, durable writes and flexible custom fields.

  Keep the registries local or run them as separate services on other machines. File contents travel directly between the storage backend and the service handling the transfer.

- 🧩 Discord limits individual file sizes. WD40-S3 splits larger objects into chunks and uploads them concurrently.

- 🔄 Signed CDN URLs expire. WD40-S3 handles URL refreshing and credentials.[^1]

- 🔐 Files need encryption before they reach Discord. Block encryption lets WD40-S3 read part of a file without decrypting the whole thing.

- 🔌 Discord's APIs need custom integrations. WD40-S3 exposes storage APIs, including S3, for existing clients.

## 📜 Project origin

This project is part of a series of Discord storage projects:

- [theking90000/discord-s3](https://github.com/theking90000/discord-s3) (2021): the first attempt at a rudimentary storage server, written in TypeScript, with AES-256 encryption and a React web interface.
- [UwUDev/discord-us](https://github.com/UwUDev/discord-us) (2023): a Rust system with multithreaded uploads, fast seeking, AES-256 encryption, and custom APIs that weren't designed for interoperability. Killed by Discord's introduction of signed URLs.
- discord-uss / wd40 (2024, private): Rust and Java, a master/worker architecture, a proprietary file system, and signed URL support on top of discord-us.
- [theking90000/rust-storage-streamer](https://github.com/theking90000/rust-storage-streamer) (2026): an async rewrite of discord-us in Rust, with download improvements and S3 support.

[^1]: Rate-limit management and prediction are deliberately excluded from the model, which assumes a large pool of bots and webhooks. This keeps the overhead of rate-limit tracking and predictive scheduling out of the transfer pipeline.
