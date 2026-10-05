# 云盘扫描的安全边界

调查日期：2026-10-02。研究未访问真实云盘。本轮随后实现了独立的 local-only 扫描模式；云盘扫描功能继续延期。本文保留原先对通用扫描器的审计，不能将它和新的保护模式混为一谈。

## 实施状态

`local_scan.rs` 提供新的保护模式，Web/HTTP 与 Tauri 已接入。macOS 每个扫描线程在首次访问前设置禁止 dataless materialization 的策略，设置失败即停止；已知云目录先做词法排除，目录遍历不跟随符号链接，跨卷下降被截断。HTTP 与桌面的发现视图都只从已测量的报告在内存中派生，不重访文件系统。全盘扫描关闭 `.gitignore` 内容读取。界面分列 allocated / logical 与系统卷容量，并显示权限拒绝和未知范围；浏览器 host 的保护模式只读，桌面壳则复用同一份报告做 trash 清理（计划期指纹基线，被跳过/不完整条目在探测前即拒绝）。

以下“当前扫描器”“通用扫描”及未实施的建议，描述的是原始审计时的旧入口。旧 generic API 和 GPUI 尚未迁移，仍不能按新的保护承诺运行。真实全盘验证结果另记于运行报告；云盘仍不扫描、不申请权限。

## 结论

当前扫描器不能承诺“扫描不会触发云文件下载”。`stat`、目录枚举和 Foundation resource values 是不同的访问边界；没有显式读取目标文件内容，不代表操作不会触发 File Provider。要满足用户严格的禁止下载要求，默认扫描应排除已知云目录、云/网络卷和未知挂载点，在首次访问前拒绝这些范围；远端容量单独使用经过用户授权的只读元数据 API。路径黑名单本身不能识别所有第三方或自定义云根，因此这是保守方案而非通用数学保证。

## Apple 官方证据：哪些操作会越界

| 操作 / 概念 | 官方资料与可得结论 |
| --- | --- |
| Dataless 文件 / 目录 | 文件可以仅有名称、大小等元数据；dataless 目录尚未枚举其内容。打开远端文件会请求 `fetchContents`；访问未枚举目录会请求 enumerator。目录元数据网络请求与下载文件内容应分别记录。 [File Provider 同步模型](https://developer.apple.com/documentation/fileprovider/synchronizing-the-file-provider-extension) |
| `stat` / `getattrlist` | Apple 建议读取 `SF_DATALESS` 检测占位项，但明确指出这些调用会 materialize 路径中的 dataless 中间目录。没有此标志也不能推断访问必然是本地 I/O，例如 NFS。 [TN3150](https://developer.apple.com/documentation/technotes/tn3150-getting-ready-for-data-less-files?language=objc) |
| 目录枚举 | TN3150 的实际调用栈展示 `contentsOfDirectoryAtPath` → `getattrlistbulk` → APFS materialization；不能将“列目录”统一描述为完全无副作用。POSIX `readdir` 文档定义的是取得下一目录项，并没有为所有 File Provider 保证零网络或零 materialization。 [TN3150](https://developer.apple.com/documentation/technotes/tn3150-getting-ready-for-data-less-files?language=objc), [Darwin readdir](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/readdir.3.html) |
| `resourceValues(forKeys:)` | Foundation 先用 URL 缓存；缺失时同步访问 backing store；未取得的值可能是 nil。这是获取元数据的 API，但文档并未给出普遍的“不触发 hydration”契约。 [Foundation resource values](https://developer.apple.com/documentation/foundation/url/resourcevalues(forkeys:)) |
| iCloud 下载状态 | `ubiquitousItemDownloadingStatusKey` 表示本地副本是否存在、是否最新；`isUbiquitousItemKey` 表示 iCloud 存储项。不能据此覆盖所有第三方 File Provider。 [iCloud 状态键](https://developer.apple.com/documentation/foundation/urlresourcekey/ubiquitousitemdownloadingstatuskey) |
| 禁止 dataless materialization | Apple 提供 `setiopolicy_np(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES, scope, IOPOL_MATERIALIZE_DATALESS_FILES_OFF)`，scope 可为线程或进程；应处理 `EDEADLK` 并恢复原策略。 [TN3150](https://developer.apple.com/documentation/technotes/tn3150-getting-ready-for-data-less-files?language=objc) Darwin 内核实现区分线程/进程策略，属于更明确的系统防线。 [Apple XNU 实现](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_resource.c) |

实现含义：可在隔离的扫描进程设置 opt-out，并对设置失败、`EDEADLK`、未知 provider 状态采取跳过/停止策略。若采用线程 guard，必须覆盖 Rayon 的实际工作线程以及任何 Foundation 执行线程；只包住调用入口线程不够。该策略针对当前访问者的 dataless materialization，不能承诺阻止独立云客户端的后台同步、NFS 网络访问或其他进程的下载。

## Google Drive：本地占用和云容量必须分开

- Stream 的文件主要存于云端，访问后会在本地可用；Mirror 保留完整本地副本。两种模式的修改都会同步到其他设备，因此普通删除不是“仅移除本地下载”。 [Google stream / mirror](https://support.google.com/drive/answer/13401938?hl=en)
- macOS 12.1 及以上的 streaming 使用 File Provider。其默认路径为 `~/Library/CloudStorage`，旧版 streaming 可在 `/Volumes/GoogleDrive`，且旧版位置可自定义。这些默认路径可以作为保守排除规则，不能作为唯一识别机制。 [Google macOS File Provider](https://support.google.com/drive/answer/12178485?hl=en)
- Drive API 的 File 是元数据资源；当前 `size` 包含 blob 和 Workspace editor 文件，文件夹和 shortcut 等没有此值。`quotaBytesUsed` 包括当前版本及保留的旧版本，所以不等于文件逻辑大小，更不等于本地占用。缺失大小必须显示未知，不能当作 0。 [Drive File resource](https://developers.google.com/workspace/drive/api/reference/rest/v3/files)
- 可用 `files.list` 分页读取 ID、父目录、类型、`size`、`quotaBytesUsed` 等元数据，构建独立云容量视图；不得请求缩略图、预览链接或内容。下载 blob 的 `files.get?alt=media` 和 export 属于另一条明确的内容路径。 [files.list](https://developers.google.com/workspace/drive/api/reference/rest/v3/files/list), [下载与导出](https://developers.google.com/workspace/drive/api/guides/manage-downloads)
- 建议为完整云容量调查请求 `drive.metadata.readonly`，避免具有下载能力的 `drive.readonly` 或具有写入能力的 scope；前者仍是受限的全盘元数据权限，必须向用户说明范围。若只访问用户选择的少数文件，应评估更窄的 `drive.file`，但它不能代表完整账号容量。 [Google OAuth scopes](https://developers.google.com/workspace/drive/api/guides/api-specific-auth)

## 大小语义

| 字段 | 解释 | 使用方式 |
| --- | --- | --- |
| 本地 logical bytes | `stat.st_size` / 文件逻辑长度。 | 与本地 allocated 分列；占位文件有逻辑长度不意味着内容已下载。 |
| 本地 allocated bytes | Darwin `st_blocks × 512` 表示实际分配块；Foundation 还提供 allocated resource keys。 | 展示为扫描时的本地分配量，不承诺逐项删除后的真实回收量。 |
| 远端 file size / quota bytes | provider 元数据字段，口径由 provider 定义。 | 放在独立远端视图，不叠加到本地磁盘占用。 |
| unknown / skipped | 未枚举、被排除、权限拒绝或没有大小字段。 | 显式标记缺失覆盖，不能伪装成空目录或 0 B。 |

Darwin 对 `st_size`、`st_blocks` 和 512 字节块的定义见 [stat 手册](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/stat.2.html)；Foundation allocated 键见 [fileAllocatedSizeKey](https://developer.apple.com/documentation/foundation/urlresourcekey/fileallocatedsizekey) 和 [totalFileAllocatedSizeKey](https://developer.apple.com/documentation/foundation/urlresourcekey/totalfileallocatedsizekey)。`lstat` 只保证最终符号链接按链接本身取信息，不意味着路径中间组件不会被解析。 [Darwin stat](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/stat.2.html)

## DaisyDisk 的借鉴与边界

DaisyDisk 官方区分“直接连接云账户扫描”和“扫描本地同步文件夹”。前者不创建文件的本地缓存；后者可能漏掉未同步文件，或需要大量本地缓存。这支持独立云元数据视图，但不能据此断言 Space Lens 当前本地递归安全。 [DaisyDisk CloudScan](https://daisydiskapp.com/guide/4/en/CloudScan/)

DaisyDisk 将受限/不可达区域、APFS 其他卷、文件系统开销等未计入树的空间单列 hidden space；purgeable 是系统可回收空间，包含快照等，和直接遍历得到的文件总量不是同一口径。建议同样解释未知/排除覆盖，而不是要求扫描所有区域来“凑齐”磁盘总量。本次不实施 purge。 [Hidden space](https://daisydiskapp.com/guide/4/en/HiddenSpace/), [Purgeable space](https://daisydiskapp.com/guide/4/en/PurgeableSpace/)

## 仓库只读审计

1. [cloud/platform.rs](../../vendors/kuntu/crates/kuntu-scan/src/cloud/platform.rs)：`inspect` 读取前后 fingerprint、分配量，调用 `isUbiquitousItemAtURL`，对目录读取 package resource，对 iCloud 文件读取下载/上传状态及 allocated resource values。未调用内容打开或 `startDownloading`，但也没有上述 opt-out；不能将这一点升级为零 hydration 保证。显式下载与 eviction 在独立方法中。
2. [cloud/guard.rs](../../vendors/kuntu/crates/kuntu-scan/src/cloud/guard.rs)：fingerprint 和 allocated 使用 `symlink_metadata`，logical 使用 `metadata.len`；这是调用边界事实，不是对 provider 行为的保证。
3. [scanner.rs](../../vendors/kuntu/crates/kuntu-scan/src/scanner.rs)：通用扫描会递归 `read_dir` / `symlink_metadata`；无云根、挂载边界、`SF_DATALESS` 或 materialization policy guard。`follow_symlinks=false` 只限制链接，不能排除普通目录形态的 File Provider 域或挂载点。`IgnoredMode::Summarize` 仍递归统计被折叠目录，因此折叠不是跳过 I/O。
4. `append_gitignore` 在 `respect_gitignore=true` 时调用 `GitignoreBuilder.add(.gitignore)`。依赖源码明确 `File::open` + `BufReader.lines`，实际读取文件内容；云占位 `.gitignore` 因而是重要 hydration 边界。禁用 ignore 读取可消除这条内容路径，但不能消除目录枚举问题。 [ignore crate 一手源码](https://docs.rs/ignore/latest/src/ignore/gitignore.rs.html#405-440)
5. 通用扫描 Unix 的 size 是 `blocks × 512`；非 Unix 回退 `metadata.len`。Windows 的 inode 去重还会 `File::open`，不能将所有平台统称为纯 metadata-only。以上均见 [scanner.rs](../../vendors/kuntu/crates/kuntu-scan/src/scanner.rs)。
6. [cloud/engine.rs](../../vendors/kuntu/crates/kuntu-scan/src/cloud/engine.rs) 的 eviction 规划也递归 `read_dir`；严谨的 eviction eligibility 检查保护的是操作选择，不证明扫描枚举本身不会下载。
7. HTTP/native discovery 的 full 和 gitignored 两遍扫描重新走 generic scanner，因此需要共享同一排除/防线，不能只保护初始 Browse 扫描。 [HTTP discovery](../../packages/host/src/discovery.ts), [native discovery](../desktop/src-tauri/src/engine.rs)

## 保守实施方案与可承诺范围

- 默认只扫描用户明确选择的本地目录；排除 cloud-root 列表及其后代、网络/未知卷。不得通过进入云域、`exists`、预览、读取 `.gitignore` 来“探测是否安全”。用户提供的云根路径直接拒绝或展示未扫描说明；扫描本地父目录时在下降前截断。
- mount/volume 元数据可辅助识别，Foundation 提供 `volumeIsLocalKey`；但“本地卷”不等于“无 File Provider”，因为 Google File Provider 可以位于用户目录。保留额外云域排除。 [Apple volumeIsLocalKey](https://developer.apple.com/documentation/foundation/urlresourcekey/volumeislocalkey), [Google 位置说明](https://support.google.com/drive/answer/12178485?hl=en)
- 未来以隔离扫描进程加系统 opt-out 作为防线，拒绝任何设置失败；显式报告 dataless/权限/网络/未知跳过。限制重试，不因失败改走内容读取或自动申请更大权限。
- 默认不用管理员或 Full Disk Access；只在用户明确需要更广本地覆盖时解释可读区域与权限用途。macOS 本身对 Documents、Desktop、Downloads、iCloud、网络和可移动卷设访问控制；授权能力不是禁止下载策略。 [Apple 文件访问控制](https://support.apple.com/en-euro/guide/security/secddd1d86a6/web)
- 可承诺的是：当前实现没有经审计的全云零 hydration 保证；未来排除模式不主动遍历已识别云根，元数据 API 模式不请求文件内容。不能承诺系统或独立 provider 后台永远不下载，也不能以未做实机实验推断每个 provider 都符合期望。
- 本地 Trash、云端删除和“移除本地下载”必须是不同操作；在当前研究范围内不执行任何一种。

## 实施前验证计划（保留审计记录）

下列准备阶段的要求已经在本轮本地扫描中实施、验证；实际结果见 [FULL_DISK_SCAN_REPORT.md](./FULL_DISK_SCAN_REPORT.md)。云盘功能继续延期，不将本地保护模式当作云存储检查器。

- 现有 Rust CLI 已用 `cargo build --release -p spacelens` 构建成功（optimized）；只编译，未用此二进制扫描全盘或云盘。release 优化不会改变上述安全边界。
- 待确认的首轮范围：Macintosh HD 本地可访问内容；Portable2TB 可另做压力测试；云根、网络卷及未知卷均跳过，不请求云盘权限，不自动提权。
- 真正启动前必须先实现并验证隔离扫描进程的 materialization opt-out、首次访问前的云范围排除、卷边界、可取消进度及结构化覆盖报告；初始扫描、展开、discovery 和后续重新计量需共用保护。现有 HTTP Node worker 不是 OS 进程隔离，不能把它当成这条防线已经生效。
- 用完全本地的测试树验证：排除根不访问、权限拒绝可见、保护设置失败即停止、硬链接去重、稀疏文件 logical/allocated 区分、多个线程/卷边界和取消。禁止以真实云目录作未经用户授权的探针。
- heavy 运行时记录时间、文件/目录计数、峰值内存、取消延迟、未扫描原因及磁盘系统容量；不强迫文件树总和等于 used space，不把重叠的 snapshots/purgeable 当成可相加类别。
