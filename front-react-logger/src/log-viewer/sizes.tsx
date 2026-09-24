import type { FileSize } from "./protocol";
export function formatBytes(value: string | null | undefined) {
  if (value == null) return "indisponible";
  const n = BigInt(value);
  const units = ["o", "Kio", "Mio", "Gio", "Tio", "Pio", "Eio"];
  let power = 1n,
    index = 0;
  while (n >= power * 1024n && index < units.length - 1) {
    power *= 1024n;
    index++;
  }
  return index === 0
    ? `${n} o`
    : `${n / power},${((n % power) * 10n) / power} ${units[index]}`;
}
export function Sizes({
  size,
  loading = false,
}: {
  size: FileSize | undefined;
  loading?: boolean;
}) {
  return (
    <span
      className="entry-sizes"
      title={
        size
          ? `Contenu : ${size.content} octets · Alloué : ${size.allocated ?? "indisponible"}${size.partial ? " · Résultat partiel" : ""}`
          : loading
            ? "Calcul en cours"
            : "Taille indisponible"
      }
    >
      {size ? (
        <>
          Contenu {formatBytes(size.content)} · Alloué{" "}
          {formatBytes(size.allocated)}
          {size.partial ? " · partiel" : ""}
        </>
      ) : loading ? (
        "Calcul en cours…"
      ) : (
        "Taille indisponible"
      )}
    </span>
  );
}
