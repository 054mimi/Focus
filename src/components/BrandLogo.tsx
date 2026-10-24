type Props = { compact?: boolean };

export function BrandLogo({ compact = false }: Props) {
  return (
    <div className={`brand-logo${compact ? " compact" : ""}`}>
      <svg className="brand-target" viewBox="0 0 48 48" role="img" aria-label="Focus target">
        <circle cx="24" cy="24" r="17" fill="none" stroke="currentColor" strokeWidth="1.5" />
        <circle cx="24" cy="24" r="10" fill="none" stroke="currentColor" strokeWidth="1.5" />
        <circle cx="24" cy="24" r="2.5" fill="currentColor" />
        <path d="M24 1v7M24 40v7M1 24h7M40 24h7" stroke="currentColor" strokeWidth="1.5" />
      </svg>
      {!compact && (
        <span className="brand-lockup">
          <strong>FOCUS — MIMI</strong>
          <small>Commit. Focus. Complete.</small>
        </span>
      )}
    </div>
  );
}
