import { registerDesktopAction } from "@/lib/desktop-control"

/** Native plugin operations used by desktop buttons, with JSON-only arguments. */
export function registerPlatformActions(): () => void {
  const stop = [
    registerDesktopAction("system.autostartStatus", "(): Promise<boolean>", async () => {
      return (await import("@tauri-apps/plugin-autostart")).isEnabled()
    }),
    registerDesktopAction("system.setAutostart", "(enabled: boolean): Promise<void>", async enabled => {
      if (typeof enabled !== "boolean") throw new Error("enabled must be a boolean")
      const autostart = await import("@tauri-apps/plugin-autostart")
      await (enabled ? autostart.enable() : autostart.disable())
      const { useWikiStore } = await import("@/stores/wiki-store")
      const { saveGeneralConfig } = await import("@/lib/project-store")
      const config = { ...useWikiStore.getState().generalConfig, autostart: enabled }
      useWikiStore.getState().setGeneralConfig(config)
      await saveGeneralConfig(config)
    }),
    registerDesktopAction("system.openUrl", "(url: string): Promise<void>", async url => {
      if (typeof url !== "string") throw new Error("url must be a string")
      await (await import("@tauri-apps/plugin-opener")).openUrl(url)
    }),
    registerDesktopAction("clipboard.read", "(): Promise<string>", () => navigator.clipboard.readText()),
    registerDesktopAction("clipboard.write", "(text: string): Promise<void>", text => {
      if (typeof text !== "string") throw new Error("text must be a string")
      return navigator.clipboard.writeText(text)
    }),
    registerDesktopAction("window.minimize", "(): Promise<void>", async () => {
      return (await import("@tauri-apps/api/window")).getCurrentWindow().minimize()
    }),
    registerDesktopAction("window.toggleMaximize", "(): Promise<void>", async () => {
      return (await import("@tauri-apps/api/window")).getCurrentWindow().toggleMaximize()
    }),
  ]
  return () => stop.forEach(unregister => unregister())
}
