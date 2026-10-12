## WD40

Store your files on Discord using object storage APIs.

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
