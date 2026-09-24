/**
 * Client half of the Space Lens plugin: the scan panel in the Harness Web UI.
 *
 * Two registrations, and neither of them draws a scan UI — the workbench itself is
 * Space Lens's own SPA, served by the host half and shown here in a frame. The one
 * thing this half owns is *which* directory the frame opens: it asks the host for
 * the current session's workbench URL rather than guessing, so the panel answers
 * "the folder this session is working in" — and the host's redirect adds
 * `autoscan`, so the scan of that folder starts on its own.
 */

window.__ModuleLoader__.load({
  id: 'dsh-plugin-spacelens',
  factory(require) {
    const React = require('react')
    const h = React.createElement

    /** Slot id, sidebar entry key and main-panel key. */
    const PANEL_ID = 'spacelens'
    /** The right-Sidebar tab type's identity; also the key its body registers under. */
    const TAB_ID = 'dsh-plugin-spacelens'
    const TAB_KIND = 'spacelens-scan'
    const NS = 'spacelens'
    /** The host half mounts the workbench here. */
    const CONTEXT_URL = '/space-lens/dsh/context'

    const DICTS: Record<string, Record<string, string>> = {
      en: {
        panel: 'Disk',
        loading: 'Opening the workbench…',
        failed: 'The Space Lens panel could not reach the workbench',
        retry: 'Retry',
        frameTitle: 'Space Lens workbench',
        openInPanel: 'Open the Space Lens workbench in the side panel',
      },
      zh: {
        panel: '磁盘',
        loading: '正在打开工作台…',
        failed: 'Space Lens 面板无法连接工作台',
        retry: '重试',
        frameTitle: 'Space Lens 工作台',
        openInPanel: '在右侧面板打开 Space Lens 工作台',
      },
    }

    /**
     * Apply-closure translate. The components are created before any plugin is
     * applied, so they cannot capture the locale service; the English dictionary
     * is the fallback for the moment before `apply` runs.
     */
    let boundT: ((key: string) => string) | null = null
    function translate(key: string): string {
      if (boundT !== null) {
        return boundT(key)
      }
      return DICTS['en']?.[key] ?? key
    }

    /** The applied context, for the components that act on the shell (opening the side tab). */
    let boundContext: HarnessClientContext | null = null

    /** The panel's square nav icon: a lens over a pie, drawn in the host's own colours. */
    function SpaceLensIcon({ size }: { size?: number }) {
      const edge = typeof size === 'number' ? size : 18
      return h(
        'svg',
        {
          viewBox: '0 0 24 24',
          width: edge,
          height: edge,
          'aria-hidden': true,
          fill: 'none',
          stroke: 'currentColor',
          style: { display: 'block' },
        },
        [
          h('circle', { key: 'lens', cx: 10.5, cy: 10.5, r: 6.5, strokeWidth: 2 }),
          h('line', {
            key: 'handle',
            x1: 15.5,
            y1: 15.5,
            x2: 21,
            y2: 21,
            strokeWidth: 2,
            strokeLinecap: 'round',
          }),
          h('path', {
            key: 'pie',
            d: 'M10.5 6.5 A4 4 0 0 1 14.5 10.5 L10.5 10.5 Z',
            fill: 'currentColor',
            stroke: 'none',
          }),
        ],
      )
    }

    /**
     * The panel: a frame on the host's workbench URL.
     *
     * The URL is fetched rather than composed because loading it is a pairing
     * redirect — the frame is authenticated by a single-use ticket the host mints
     * per document load — and because only the host knows which directory this
     * Session is about. `sessionId` is what makes a right-column tab belong to
     * the project it was opened in.
     */
    function SpaceLensPanel({ sessionId }: { sessionId?: unknown }) {
      const [frameUrl, setFrameUrl] = React.useState<string | null>(null)
      const [failure, setFailure] = React.useState<string | null>(null)
      const [attempt, setAttempt] = React.useState(0)

      React.useEffect(() => {
        let live = true
        setFailure(null)
        const query = new URLSearchParams()
        if (typeof sessionId === 'string' && sessionId !== '') {
          query.set('session', sessionId)
        }
        const suffix = query.size === 0 ? '' : `?${query.toString()}`
        fetch(`${CONTEXT_URL}${suffix}`, { headers: { accept: 'application/json' } })
          .then((response) => {
            if (!response.ok) {
              throw new Error(`context request answered ${String(response.status)}`)
            }
            return response.json() as Promise<{ panelUrl?: unknown }>
          })
          .then((payload) => {
            if (live) {
              setFrameUrl(String(payload.panelUrl))
            }
          })
          .catch((error: unknown) => {
            if (live) {
              setFailure(error instanceof Error ? error.message : String(error))
            }
          })
        return () => {
          live = false
        }
      }, [attempt, sessionId])

      if (failure !== null) {
        return h(
          'div',
          { style: { display: 'grid', placeItems: 'center', height: '100%', gap: 12 } },
          h('p', { style: { margin: 0, opacity: 0.8 } }, `${translate('failed')} (${failure})`),
          h(
            'button',
            {
              type: 'button',
              onClick: () => setAttempt((value) => value + 1),
              style: {
                padding: '6px 14px',
                borderRadius: 8,
                border: '1px solid rgba(128,128,128,.4)',
                background: 'transparent',
                color: 'inherit',
                cursor: 'pointer',
              },
            },
            translate('retry'),
          ),
        )
      }

      if (frameUrl === null) {
        return h(
          'div',
          { style: { display: 'grid', placeItems: 'center', height: '100%', opacity: 0.7 } },
          h('p', { style: { margin: 0 } }, translate('loading')),
        )
      }

      return h('iframe', {
        src: frameUrl,
        title: translate('frameTitle'),
        style: {
          display: 'block',
          width: '100%',
          height: '100%',
          border: 'none',
          background: 'transparent',
        },
      })
    }

    /**
     * The Session-header control that reveals the scan tab in the right column.
     *
     * This is the seat that answers "the folder of whichever project is open": the
     * right column is Session-scoped, so a project that has opened the workbench
     * keeps it, scanning that project's directory — not whichever one was opened
     * last anywhere.
     */
    function SpaceLensHeaderButton() {
      const open = () => {
        boundContext?.get('sidebarRight')?.openTab(TAB_KIND)
      }
      return h(
        'button',
        {
          type: 'button',
          onClick: open,
          title: translate('openInPanel'),
          'aria-label': translate('openInPanel'),
          style: {
            display: 'inline-flex',
            alignItems: 'center',
            justifyContent: 'center',
            width: 28,
            height: 28,
            padding: 0,
            borderRadius: '50%',
            border: 'none',
            background: 'transparent',
            color: 'inherit',
            cursor: 'pointer',
          },
        },
        h(SpaceLensIcon, { size: 16 }),
      )
    }

    return {
      inject: ['slots', 'locale'],
      apply(ctx: HarnessClientContext) {
        ctx.effect(() => {
          ctx.locale.register(NS, DICTS)
        }, 'spacelens: dictionaries')
        boundT = ctx.locale.bind(NS)
        boundContext = ctx
        ctx.slots.inject('main', () => ctx.slots.register({ name: 'main', key: PANEL_ID, locale: NS }, SpaceLensPanel))
        ctx.slots.inject('sidebar.panellist', () =>
          ctx.slots.register(
            {
              name: 'sidebar.panellist',
              id: PANEL_ID,
              order: 71,
              label: () => translate('panel'),
              locale: NS,
            },
            SpaceLensIcon,
          ),
        )

        // The right column is the Session-scoped copy of the same workbench. Registered
        // through `ctx.inject` rather than read with `ctx.get`, because the registry
        // belongs to another plugin: a plain read inside `apply` is a race with
        // activation order, and losing it skips every registration silently.
        ctx.inject(['sidebarRightTabs'], (scoped: HarnessInjectedContext) => {
          scoped.effect(() => {
            scoped.sidebarRightTabs.register({
              id: TAB_ID,
              kind: TAB_KIND,
              title: () => translate('panel'),
            })
          }, 'spacelens: right-column tab type')
          scoped.slots.inject('sidebar.right.pane.tab', () =>
            scoped.slots.register({ name: 'sidebar.right.pane.tab', key: TAB_ID, locale: NS }, SpaceLensPanel),
          )
          scoped.slots.inject('conversation.session.header.utilities', () =>
            scoped.slots.register(
              {
                name: 'conversation.session.header.utilities',
                id: PANEL_ID,
                order: 26,
                label: () => translate('panel'),
                locale: NS,
              },
              SpaceLensHeaderButton,
            ),
          )
        })
      },
    }
  },
})
