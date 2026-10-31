# Quick self-host guide

This guide explains how to host the S3 system using the embedded server and portal.

This guides provides a guided path into configure and installing WD40-S3 server.

## 1. Prepare Discord webhooks

You will need to generate a large amount of discord webhooks (more is always better, I would say at least 10 webhooks but haven't done any measurements, I use 85 webhooks for my demo). 

The system currently doens't handle webhook rate-limiting for performance reasons. So it uses the assumption that a large number of webhook will be used so that rate limits will never be hit.

Go on a discord account (preferably not your main account) and create discord servers, with each one containing one channel with one webhook. It is preferable that the webhook are created accross differents discord servers (or at least channels) for rate limits and spreading purposes. Using the same account should not be a problem.

If you don't know how to create a webhook, please refer to the guide [Create a webhook](./create-webhook.md).

Copy and store the URL of the webhooks you created in a separate file, since you will need it later to configure the WD40-S3 server.

## 2. Prepare the bot token

You will also need a bot token to make WD40-S3 work properly. This bot is purely used for signed URL refreshing and the Discord bot doens't need to be installed on any server nor to be connected to any gateway, we just need its token.

Go to [https://discord.com/developers/applications](https://discord.com/developers/applications) and create an new applications, then go directly to the Bot section and copy the bot token. Do not forget to store it a file, you will need it later.

Refer to [Create a bot token](./create-bot-token.md) if you need guidance in how to create the bot token.

## 3. Prepare the OpenID Connect (OIDC) provider

This is the trickiest part and the less intuitive one if you have never worked with OIDC/oauth2. 

Currently, the WD40-S3 portal, which allows user to self-manage their S3 buckets and keys, doens't implement an user management system, instead it relies on a third party providers supporting the standard OIDC protocol.

On my demo server I use [rauthy](https://github.com/sebadob/rauthy) as my identity provider. But you actually don't need to setup a instance and you can use your already existing infrastructure (if you have one) or simply connect it to an existing third party.

What you need is
- The OIDC Issuer URL
- An OIDC Client ID
- An OIDC Client Secret.

Unfortunately, Discord only supports OAuth2 login (not full OIDC compliance) so it's not useable "as is" for portal authentification. There are some conversion layers : [https://github.com/taiseiue/discord-oidc-proxy](https://github.com/taiseiue/discord-oidc-proxy) but I haven't look into it.

However Google can be used as an OIDC provider (as well as platform like auth0, clerk, ... if you do not want to self host too). The guide to create your client ID/Secret is availble here : [Create a Google OIDC Client](./create-google-oidc-client)

> Please note that you need to configure the allowed callback URL list of your OIDC Client to include the URL where the console will be deployed. For instance if your console will be available via url `https://console.example.com`, the callback URL should allow `https://console.example.com/*`.

## 4. Get the WD40-S3 server binary

### 4.1 Application components

To run the full applications you need several components : 

- WD40 engine components (3) : file-registry, stream-registry, blob-registry.
- WD40 object layer (3) : object-registry, policy-registry, metrics-registry.
- WD40 S3 layer (3) : s3-adapter, s3-credentials-registry, s3-multipart-registry.
- WD40 S3 portal (2) : portal, web-console.

Each component may be run as separate process over network via gRPC or directly embedded into one monolith executable. 

For now, I recommend using the monolith way, as this achieves best performance under constraint ressources environement (expect 80 to 200MB of ram usage for the whole monolith process for heavy parrallel downloads of multiple Gbit/s depending on your buffering settings).

### 4.2 Precompiled executable

All components are packaged into a single monolith executable, which is automatically built for **linux x86-64** and **linux arm64** using github actions. Check Github action to download the latest executable here : [https://github.com/theking90000/WD40-S3/actions/workflows/portal-linux.yml](https://github.com/theking90000/WD40-S3/actions/workflows/portal-linux.yml).

To install it, download the zip on the github interface. (Unfortunately Github requires authentication to download actions artifacts, so this can't be done in automated/CLI environnement without a proper github token).

Unzip it, you will find a correspoding "portal-linux-{arch}.tar.gz" tar file, which containing a single binary named `portal`, that you can use to run the server.

### 4.3 Compile yourself

If somehow the executable don't work on your machine (glibc incompatibility, different OS or achitecture, or not able to download the github action artifact). You can compile yourself the executable by cloning the git repo.

For this you will need 
- A valid compiling environnement (base-devel and stuff)
- Modern Rustc and Cargo (1.85.0+) installed
- Npm installed

```sh
# Assume you have already cloned the repo, and rustc,cargo and npm intsalled
cargo build -p portal --release --features standalone,embedded-s3

# If you are on x86-64 linux, rocksDB might require this flag before compiling
# if it fails, retry with this environnement variable
export CXXFLAGS=-DNO_PCLMUL
```

The binary will be located in `target/release/portal`.

## 5. Server configuration

TODO

## 6. Deployment

TODO

<!-- 

## 3. Configure and start

Create a private configuration file:

```sh
umask 077
touch wd40.env
chmod 600 wd40.env
```

Edit `wd40.env` and replace the placeholders:

```sh
WD40_PORTAL_OIDC_ISSUER_URL='https://identity.example.com'
WD40_PORTAL_OIDC_CLIENT_ID='wd40-portal'
WD40_PORTAL_OIDC_CLIENT_SECRET='REPLACE_WITH_CLIENT_SECRET'
WD40_DISCORD_WEBHOOK_URLS='https://discord.com/api/webhooks/REPLACE_ID/REPLACE_TOKEN'
WD40_DISCORD_BOT_TOKENS='REPLACE_WITH_BOT_TOKEN'
WD40_PORTAL_EMBEDDED_S3=true
WD40_PORTAL_S3_HOST='s3.example.com'
WD40_PORTAL_LISTEN='127.0.0.1:8080'
WD40_ENCRYPTION=aes256
```

`WD40_PORTAL_S3_HOST` is a hostname without a scheme or path. Encryption defaults
to `none`; this configuration enables AES-256 for new uploads.

Load the settings and run the server:

```sh
set -a
. ./wd40.env
set +a
./portal
```

The first start creates `portal.db` and the registry directories under `data/`
in the working directory. The console and S3 share `127.0.0.1:8080`, with requests
routed by their public `Host` header. Embedded S3 also records usage metrics.

## 4. Serve HTTPS

Forward both domains to `127.0.0.1:8080`, preserving the original `Host`, request
path and query string. The public host is required for S3 routing, request
signatures and secure login cookies.

For example, with Caddy running on the same server:

```text
console.example.com {
    reverse_proxy 127.0.0.1:8080
}

s3.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

Check the portal through the proxy:

```sh
curl --fail https://console.example.com/api/health
```

It should return `ok`. With these domain names, the console automatically displays
`https://s3.example.com` as the S3 endpoint. A different naming scheme requires
setting `VITE_S3_ENDPOINT` when building the executable.

## 5. Upload your first object

Open `https://console.example.com/`, sign in, create an organization, then create
a bucket and an access key with access to it. Save the secret when it appears;
the console shows it only once.

Configure an AWS CLI profile with that access key, secret and region `us-east-1`:

```sh
aws configure --profile wd40
aws configure set s3.addressing_style path --profile wd40
printf 'Hello from WD40-S3\n' > hello.txt
aws --profile wd40 --endpoint-url https://s3.example.com \
  s3 cp hello.txt s3://YOUR_BUCKET/hello.txt
aws --profile wd40 --endpoint-url https://s3.example.com \
  s3 cp s3://YOUR_BUCKET/hello.txt -
```

Path-style addressing works with the two DNS records above. Virtual-hosted
requests such as `YOUR_BUCKET.s3.example.com` also need wildcard DNS, TLS and
proxy routing for `*.s3.example.com`.

## Keep the metadata

Run the executable under your service manager with the same working directory
and environment on every start. Use Ctrl-C for graceful shutdown when running
interactively. Keep `wd40.env` private and back up `portal.db` and the entire
`data/` directory while the service is stopped. The registries contain the
metadata and encryption keys needed to retrieve your files from Discord.

Run one process per set of local registry directories. To split services across
processes, use shared registries over gRPC. See the
[portal reference](https://github.com/theking90000/WD40-S3/blob/main/portal/README.md)
for remote registries, separate listeners and other deployment options. -->
