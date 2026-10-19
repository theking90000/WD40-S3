import { useEffect, useState } from "react";
import { CopyValue } from "~/components/copy-value";
import { resolveS3Endpoint, s3BucketUrl, S3_REGION } from "~/lib/s3.ts";

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
  const bucketUrl = endpoint && bucketName ? s3BucketUrl(endpoint, bucketName) : null;
  const Heading = bucketName ? "h3" : "h2";

  return (
    <div className={compact ? "space-y-3" : "space-y-3 rounded-xl border bg-surface p-4"}>
      <Heading className="text-xs font-semibold">Connexion S3</Heading>
      {endpoint === undefined ? <p role="status" className="text-xs text-muted-foreground">Chargement de l'endpoint…</p>
        : endpoint === null ? <p className="text-xs text-muted-foreground">L'endpoint S3 n'est pas configuré pour cette adresse de console. Demandez l'adresse à votre administrateur.</p>
          : <>
            <dl className={compact ? "space-y-3" : "grid gap-3 min-[621px]:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]"}>
              {bucketName ? <ConnectionValue label={bucketUrl ? "URL du bucket" : "Endpoint"} value={bucketUrl ?? endpoint} /> : <>
                <ConnectionValue label="Endpoint" value={endpoint} />
                <ConnectionValue label="Région" value={S3_REGION} />
              </>}
            </dl>
            {bucketName
              ? <p className="text-[10.5px] text-muted-foreground">Région du client : {S3_REGION}.</p>
              : <p className="text-[10.5px] text-muted-foreground">Utilisez {S3_REGION} dans votre client. WD40 n'impose pas de région.</p>}
          </>}
    </div>
  );
}
