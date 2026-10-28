/* Athenaeum UI. MIT. Sync with node design-system/sync.mjs in athenaeum_library. */
import type { ReactNode } from 'react';

export function Brand({ section }: { section?: string }) {
  return (
    <span className="ath-brand">
      <svg
        viewBox="0 0 32 32"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d="M8 4h18v24H8a4 4 0 0 1 0-8h18M8 4a3 3 0 0 0-3 3v17M12 9h9M12 13h6" />
      </svg>
      <span>Athenaeum{section && <span>{section}</span>}</span>
    </span>
  );
}

export function AccountIdentity({
  username,
  detail,
}: {
  username: string;
  detail: string;
}) {
  return (
    <span className="ath-identity">
      <span className="ath-avatar" aria-hidden="true">
        {Array.from(username)[0]?.toUpperCase()}
      </span>
      <span className="ath-identity-copy">
        <strong>{username}</strong>
        <small>{detail}</small>
      </span>
    </span>
  );
}

export function PageHeading({
  title,
  description,
  actions,
  className = '',
}: {
  title: string;
  description?: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <header className={`ath-page-heading ${className}`}>
      <div>
        <h1>{title}</h1>
        {description && <p>{description}</p>}
      </div>
      {actions && <div className="ath-page-actions">{actions}</div>}
    </header>
  );
}

export function AuthFrame({
  section = 'Library',
  title = 'Your knowledge, connected.',
  description = 'Keep your wiki local. Share the pages you choose, with the people you choose.',
  children,
}: {
  section?: string;
  title?: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div className="ath-auth">
      <aside className="ath-auth-story">
        <Brand section={section} />
        <h2>{title}</h2>
        <p>{description}</p>
        <ul className="ath-auth-facts">
          <li>
            <FactIcon kind="lock" />
            Private by default
          </li>
          <li>
            <FactIcon kind="file" />
            Plain Markdown. Always yours.
          </li>
          <li>
            <FactIcon kind="history" />A history for every sync
          </li>
        </ul>
      </aside>
      <div className="ath-auth-content">{children}</div>
    </div>
  );
}

function FactIcon({ kind }: { kind: 'lock' | 'file' | 'history' }) {
  const paths = {
    lock: 'M7 10V7a5 5 0 0 1 10 0v3M5 10h14v11H5Zm7 4v3',
    file: 'M5 3h9l5 5v13H5ZM14 3v6h5M8 13h8M8 17h6',
    history: 'M3 11a9 9 0 1 1 2 7M3 4v7h7M12 7v6l4 2',
  };
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[kind]} />
    </svg>
  );
}
