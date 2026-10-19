export const S3_REGION = "us-east-1";

export function parseS3Endpoint(value: string): string {
  const endpoint = value.trim();
  const message = "VITE_S3_ENDPOINT must be an HTTP or HTTPS origin without credentials, a path, query parameters or a fragment";
  let url: URL;
  try {
    url = new URL(endpoint);
  } catch {
    throw new Error(message);
  }
  if (!/^https?:\/\//i.test(endpoint) || !["http:", "https:"].includes(url.protocol)
    || url.username || url.password || url.pathname !== "/" || endpoint.includes("?")
    || endpoint.includes("#") || endpoint.includes("@") || endpoint.includes("\\")) {
    throw new Error(message);
  }
  return url.origin;
}

function isDomain(hostname: string): boolean {
  const domain = hostname.replace(/\.$/, "");
  return domain !== "localhost" && !domain.endsWith(".localhost")
    && !domain.includes(":") && !/^[\d.]+$/.test(domain);
}

export function resolveS3Endpoint(configured: string | undefined, hostname: string): string | null {
  if (configured?.trim()) return parseS3Endpoint(configured);
  const domain = hostname.toLowerCase().replace(/\.$/, "");
  if (!domain || !isDomain(domain)) return null;
  return `https://s3.${domain.replace(/^console\./, "")}`;
}

export function s3BucketUrl(endpoint: string, bucket: string): string | null {
  const url = new URL(endpoint);
  if (!isDomain(url.hostname)) return null;
  url.hostname = `${bucket}.${url.hostname}`;
  return url.origin;
}
