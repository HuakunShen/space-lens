/**
 * Ambient declarations for the surface a Harness Web plugin bundle is loaded into.
 *
 * This plugin is loaded *by* the Harness, so it is built against nothing: the browser
 * module table it draws React from and the slot/locale services it registers into have no
 * installed types to import. Declaring the slice it uses is what lets the client half be
 * type-checked at all, and keeps the assumption visible rather than implied by an untyped
 * `require`.
 */

/** One bundle registration the Web shell's loader accepts. */
interface HarnessModuleLoaderEntry {
  readonly id: string
  readonly factory: (require: (name: string) => HarnessReact) => unknown
}

interface Window {
  /** The Web shell's bundle table; every shipped and third-party bundle registers here. */
  readonly __ModuleLoader__: {
    load(entry: HarnessModuleLoaderEntry): void
  }
}

/** The React entry points this plugin uses, from the browser module table. */
interface HarnessReact {
  createElement(type: unknown, props?: unknown, ...children: unknown[]): unknown
  useState<T>(initial: T): readonly [T, (next: T | ((current: T) => T)) => void]
  useEffect(effect: () => void | (() => void), deps?: readonly unknown[]): void
}

/** The locale service: one dictionary per namespace, and a bound translator. */
interface HarnessLocale {
  register(namespace: string, dictionaries: Readonly<Record<string, Readonly<Record<string, string>>>>): unknown
  bind(namespace: string): (key: string) => string
}

/** The slot service: an injected registration, or one made inside an injection. */
interface HarnessSlots {
  inject(name: string, register: () => unknown): unknown
  register(options: Readonly<Record<string, unknown>>, component: unknown): unknown
}

/**
 * Stage one of a right-Sidebar tab type: what the type is.
 *
 * Declared from the right Sidebar's published contract. `id` is both this registration's
 * identity and the key its body registers under in the `sidebar.right.pane.tab` seat, which
 * is why the two halves of a tab type cannot disagree about which tab they are.
 */
interface HarnessSidebarRightTabs {
  register(definition: {
    readonly id: string
    readonly kind: string
    readonly title: (address: string) => string
    /** Keep the body mounted through hiding, Session changes and docking. */
    readonly keepMounted?: boolean
  }): unknown
}

/** The right Sidebar's navigation face: opening a page type by kind reveals the column. */
interface HarnessSidebarRight {
  openTab(kind: string): void
}

/** The client plugin context handed to a bundle's `apply`. */
interface HarnessClientContext {
  readonly locale: HarnessLocale
  readonly slots: HarnessSlots
  /**
   * The right Sidebar's services, when this composition has that column.
   *
   * Read at the moment of use rather than treated as a prerequisite: the main panel is
   * useful without a right column, so a missing seat must not take the whole plugin down.
   */
  get(name: 'sidebarRight'): HarnessSidebarRight | undefined
  get(name: 'sidebarRightTabs'): HarnessSidebarRightTabs | undefined
  /**
   * Run `install` once the named services are available, and again whenever that set
   * changes.
   *
   * This is what depending on another plugin's *service* looks like. A plain
   * `ctx.get(name)` inside `apply` is a race against activation order, and losing that race
   * drops every registration after it silently and permanently — a plugin that looks like
   * it was never installed at all.
   */
  inject(names: readonly string[], install: (scoped: HarnessInjectedContext) => void): unknown
  effect(install: () => void | (() => void), label?: string): unknown
}

/** The context a plugin body receives once every service it named is available. */
interface HarnessInjectedContext extends HarnessClientContext {
  readonly sidebarRight: HarnessSidebarRight
  readonly sidebarRightTabs: HarnessSidebarRightTabs
}
