declare module "virtual:desktop-catalog" {
  export const catalog: import("./types/desktop-control").DesktopCatalog
  export const modules: Record<string, () => Promise<Record<string, unknown>>>
}
