export type Breadcrumb = {
  label: string;
  href: string;
  current: boolean;
};

function segmentLabel(segment: string) {
  return decodeURIComponent(segment).replace(/^\w/, (c) => c.toUpperCase());
}

export function buildBreadcrumbs(pathname: string): Breadcrumb[] {
  const parts = pathname.split("/").filter(Boolean);

  return parts.map((part, index) => ({
    label: segmentLabel(part),
    href: `/${parts.slice(0, index + 1).map(encodeURIComponent).join("/")}`,
    current: index === parts.length - 1,
  }));
}

export function getRouteName(pathname: string) {
  const lastSegment = pathname.split("/").filter(Boolean).at(-1);
  return lastSegment ? segmentLabel(lastSegment) : "";
}
