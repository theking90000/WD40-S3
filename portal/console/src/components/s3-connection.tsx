import { useEffect, useState } from "react";
import { CopyValue } from "~/components/copy-value";
import { resolveS3Endpoint, s3BucketUrls, S3_REGION, supportsVirtualHosting } from "~/lib/s3.ts";

function ConnectionValue({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 space-y-1">
      <dt className="text-[10.5px] text-muted-foreground">{label}</dt>
      <dd className="flex min-w-0 items-start gap-2">
        <code className="min-w-0 flex-1 select-all break-all py-1.5 font-mono text-xs">{value}</code>
        <CopyValue key={value} value={value} label={`Copier ${label.toLocaleLowerCase("fr")}`} compact />
      </dd>
    </div>
  );
}

export function S3Connection({ bucketName, compact = false }: { bucketName?: string; compact?: boolean }) {
  const [endpoint, setEndpoint] = useState<string | null>();
  useEffect(() => {
    setEndpoint(resolveS3Endpoint(import.meta.env.VITE_S3_ENDPOINT, window.location.hostname));
  }, []);
  const urls = endpoint && bucketName ? s3BucketUrls(endpoint, bucketName) : null;
  const Heading = bucketName ? "h3" : "h2";

  return (
    <div className={compact ? "space-y-3" : "space-y-3 rounded-xl border bg-surface p-4"}>
      <Heading className="text-xs font-semibold">Connexion S3</Heading>
      {endpoint === undefined ? <p role="status" className="text-xs text-muted-foreground">Chargement de l'endpoint…</p>
        : endpoint === null ? <p className="text-xs text-muted-foreground">L'endpoint S3 n'est pas configuré pour cette adresse de console. Demandez l'adresse à votre administrateur.</p>
          : <>
            <dl className={compact ? "space-y-3" : "grid gap-3 min-[621px]:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]"}>
              {urls ? <>
                {urls.virtualHosted && <ConnectionValue label="URL du bucket · virtual-hosted" value={urls.virtualHosted} />}
                <ConnectionValue label="URL du bucket · path-style" value={urls.pathStyle} />
              </> : <>
                <ConnectionValue label="Endpoint" value={endpoint} />
                <ConnectionValue label="Région" value={S3_REGION} />
              </>}
            </dl>
            {bucketName ? <>
              <p className="text-[10.5px] text-muted-foreground">Ces URL utilisent les droits d'accès du bucket. Région du client : {S3_REGION}.</p>
              {new URL(endpoint).protocol === "https:" && bucketName.includes(".") && <p className="text-[10.5px] text-muted-foreground">Utilisez le mode path-style pour ce nom contenant des points. Un certificat wildcard ordinaire ne couvre pas son sous-domaine.</p>}
            </> : <>
              <p className="text-[10.5px] text-muted-foreground">Utilisez {S3_REGION} dans votre client. WD40 n'impose pas de région.</p>
              {!compact && <p className="text-[10.5px] text-muted-foreground">{supportsVirtualHosting(endpoint)
                ? "Adressage virtual-hosted ou path-style. Le mode virtual-hosted nécessite le DNS et le certificat des sous-domaines de buckets."
                : "Utilisez le mode path-style avec cet endpoint local ou cette adresse IP."}</p>}
            </>}
          </>}
    </div>
  );
}
