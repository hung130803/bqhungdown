/**
 * Inline SVG logo for BQHungDown — a clean download arrow on a rounded gradient.
 * Renders at any size via the `size` prop.
 */
export function Logo({ size = 28, className = "" }: { size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      aria-label="BQHungDown"
    >
      <defs>
        <linearGradient id="bqhungdown-grad" x1="0" y1="0" x2="64" y2="64" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#6366f1" />
          <stop offset="100%" stopColor="#a855f7" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="14" fill="url(#bqhungdown-grad)" />
      <path
        d="M32 14v26m0 0l-10-10m10 10l10-10"
        stroke="white"
        strokeWidth="5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <rect x="14" y="46" width="36" height="5" rx="2.5" fill="white" />
    </svg>
  );
}
