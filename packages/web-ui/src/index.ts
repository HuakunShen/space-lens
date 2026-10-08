// Lens components (product)
export { default as BreadcrumbBar } from './components/lens/BreadcrumbBar.svelte'
export { default as ChildList } from './components/lens/ChildList.svelte'
export { default as CollectorPanel } from './components/lens/CollectorPanel.svelte'
export { default as ScanPicker } from './components/lens/ScanPicker.svelte'
export { default as StatusBar } from './components/lens/StatusBar.svelte'
export { default as SunburstChart } from './components/lens/SunburstChart.svelte'
export { default as TreemapChart } from './components/lens/TreemapChart.svelte'
export { default as ChartModeToggle } from './components/lens/ChartModeToggle.svelte'
export { default as LensSelect } from './components/lens/LensSelect.svelte'
export { default as ScanCoveragePanel } from './components/lens/ScanCoveragePanel.svelte'
export { default as WorkspaceHeading } from './components/lens/WorkspaceHeading.svelte'
export { default as LensToolbar } from './components/lens/LensToolbar.svelte'
export { default as ScanSidebar } from './components/lens/ScanSidebar.svelte'

export { default as DiscoveryList } from './components/lens/DiscoveryList.svelte'
export { default as AppearancePanel } from './components/lens/AppearancePanel.svelte'

// App-shell components (connection + feedback)
export { default as ConnectionPanel } from './components/app/ConnectionPanel.svelte'
export { default as StateBanner } from './components/app/StateBanner.svelte'

// Primitives, namespaced: several export the same member names (Header,
// Footer, …), so a blanket `export *` would be an ambiguity, not a convenience.
import * as Badge from './components/ui/badge/index.js'
import * as Button from './components/ui/button/index.js'
import * as Card from './components/ui/card/index.js'
import * as ContextMenu from './components/ui/context-menu/index.js'
import * as Checkbox from './components/ui/checkbox/index.js'
import * as Input from './components/ui/input/index.js'
import * as Progress from './components/ui/progress/index.js'
import * as ScrollArea from './components/ui/scroll-area/index.js'
import * as Separator from './components/ui/separator/index.js'
import * as Select from './components/ui/select/index.js'
import * as Sheet from './components/ui/sheet/index.js'
export { Badge, Button, Card, ContextMenu, Checkbox, Input, Progress, ScrollArea, Separator, Select, Sheet }

// Helpers
export { formatBytes, formatPercent } from './lib/format.js'
export { formatNodeName } from './lib/node-size.js'
export { cn } from './lib/utils.js'
export type { ChartMode } from './lib/treemap.js'
