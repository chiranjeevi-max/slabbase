import type { ReactElement, SVGProps } from "react";

export type IconName =
  | "table"
  | "form"
  | "search"
  | "plus"
  | "minus"
  | "x"
  | "check"
  | "undo"
  | "redo"
  | "sun"
  | "moon"
  | "database"
  | "download"
  | "upload"
  | "printer"
  | "history"
  | "trash"
  | "chevronDown"
  | "chevronRight"
  | "chevronUp"
  | "chevronLeft"
  | "folder"
  | "shield"
  | "grid"
  | "sliders"
  | "arrowUp"
  | "arrowDown"
  | "spark"
  | "keyboard"
  | "panelRight"
  | "sidebar"
  | "copy";

const paths: Record<IconName, ReactElement> = {
  table: (
    <>
      <rect x="2.5" y="3" width="11" height="10" rx="1.5" />
      <path d="M2.5 6.5h11M6.5 6.5V13M10 6.5V13" />
    </>
  ),
  form: (
    <>
      <rect x="2.5" y="2.5" width="11" height="11" rx="1.5" />
      <path d="M5 5.5h6M5 8h6M5 10.5h3.5" />
    </>
  ),
  search: (
    <>
      <circle cx="7" cy="7" r="4" />
      <path d="m10.5 10.5 3 3" />
    </>
  ),
  plus: <path d="M8 3v10M3 8h10" />,
  minus: <path d="M3 8h10" />,
  x: <path d="m4 4 8 8M12 4l-8 8" />,
  check: <path d="m3.5 8.5 3 3 6-7" />,
  undo: <path d="M6 4 3 7l3 3M3 7h6.5a3.5 3.5 0 0 1 0 7H8" />,
  redo: <path d="m10 4 3 3-3 3M13 7H6.5a3.5 3.5 0 0 0 0 7H8" />,
  sun: (
    <>
      <circle cx="8" cy="8" r="3" />
      <path d="M8 1.5v1M8 13.5v1M1.5 8h1M13.5 8h1M3.4 3.4l.7.7M11.9 11.9l.7.7M12.6 3.4l-.7.7M4.1 11.9l-.7.7" />
    </>
  ),
  moon: <path d="M13.2 9.5A5.5 5.5 0 0 1 6.5 2.8a5.5 5.5 0 1 0 6.7 6.7Z" />,
  database: (
    <>
      <ellipse cx="8" cy="3.8" rx="5" ry="2" />
      <path d="M3 3.8v8.4c0 1.1 2.2 2 5 2s5-.9 5-2V3.8M3 8c0 1.1 2.2 2 5 2s5-.9 5-2" />
    </>
  ),
  download: <path d="M8 2v8m0 0 3-3m-3 3L5 7M3 13h10" />,
  upload: <path d="M8 14V6m0 0 3 3M8 6 5 9M3 2.5h10" />,
  printer: (
    <>
      <path d="M5 6V2.5h6V6M5 11.5H3.5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h9a1 1 0 0 1 1 1v3.5a1 1 0 0 1-1 1H11" />
      <rect x="5" y="9.5" width="6" height="4" />
    </>
  ),
  history: (
    <>
      <path d="M2.5 8a5.5 5.5 0 1 0 1.7-4" />
      <path d="M2.5 2.5V5H5M8 5v3.2l2.2 1.3" />
    </>
  ),
  trash: (
    <>
      <path d="M3 4.5h10M6 4.5V2.8h4v1.7M4.5 4.5l.6 9h5.8l.6-9M6.8 7v4M9.2 7v4" />
    </>
  ),
  chevronDown: <path d="m4 6 4 4 4-4" />,
  chevronUp: <path d="m4 10 4-4 4 4" />,
  chevronRight: <path d="m6 4 4 4-4 4" />,
  chevronLeft: <path d="m10 4-4 4 4 4" />,
  folder: <path d="M2.5 4.5V12a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1V5.5a1 1 0 0 0-1-1H8L6.5 3H3.5a1 1 0 0 0-1 1Z" />,
  shield: <path d="M8 2.2 3.2 4v4.1c0 2.9 2 4.9 4.8 5.8 2.8-.9 4.8-2.9 4.8-5.8V4L8 2.2Zm-2 6 1.5 1.5 2.8-3" />,
  grid: (
    <>
      <rect x="2.5" y="2.5" width="4.5" height="4.5" />
      <rect x="9" y="2.5" width="4.5" height="4.5" />
      <rect x="2.5" y="9" width="4.5" height="4.5" />
      <rect x="9" y="9" width="4.5" height="4.5" />
    </>
  ),
  sliders: <path d="M3 5h4M9 5h4M3 11h7M12 11h1M7 3v4M10 9v4" />,
  arrowUp: <path d="M8 13V3m0 0L4.5 6.5M8 3l3.5 3.5" />,
  arrowDown: <path d="M8 3v10m0 0 3.5-3.5M8 13 4.5 9.5" />,
  spark: <path d="M8 2v3M8 11v3M2 8h3M11 8h3M4 4l2 2M10 10l2 2M12 4l-2 2M6 10l-2 2" />,
  keyboard: (
    <>
      <rect x="1.8" y="4" width="12.4" height="8" rx="1.4" />
      <path d="M4 6.5h.01M6.5 6.5h.01M9 6.5h.01M11.5 6.5h.01M4.5 9.5h7" />
    </>
  ),
  panelRight: (
    <>
      <rect x="2.5" y="3" width="11" height="10" rx="1.5" />
      <path d="M9.5 3v10" />
    </>
  ),
  sidebar: (
    <>
      <rect x="2.5" y="3" width="11" height="10" rx="1.5" />
      <path d="M6.5 3v10" />
    </>
  ),
  copy: (
    <>
      <rect x="5" y="5" width="8" height="8" rx="1.5" />
      <path d="M3 10.5V4a1.5 1.5 0 0 1 1.5-1.5H10" />
    </>
  ),
};

export function Icon({
  name,
  size = 16,
  ...props
}: { name: IconName; size?: number } & Omit<SVGProps<SVGSVGElement>, "name">) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.4}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      {paths[name]}
    </svg>
  );
}
