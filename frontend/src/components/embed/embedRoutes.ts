export function isEmbedPath(pathname: string): boolean {
  return pathname === '/embed' || pathname.startsWith('/embed/')
}
