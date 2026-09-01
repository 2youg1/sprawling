# memory-SPEC.md

> crate：`memory`。本 SPEC 先于代码存在；实现不多不少地遵守本文。
> 骨架：十七节；按模块分章。Stage 1 三模块（jsonl／fault_fs／cas，§8-1…§8-3）；
> Stage 3 七模块（index／hot／projection／attribution／checkpoint／queue／digest_cache，§8-4…§8-10）。

## 1 需求拆解

| 卡 | 模块 | 形状 | 一句话 |
|---|---|---|---|
| S1.07 | `jsonl` | 4 适配器 | kernel Ledger 的落盘实现：内缝 Vfs＋组提交＋断尾＋版本方向判定＋分段滚动 |
| S1.08 | `fault_fs` | 4 适配器 | Vfs 第二适配器：撕裂写／乱序持久化／rename 中断；断电点阵的驱动器 |
| S1.09 | `cas` | 4 适配器 | BLAKE3 寻址存储：范围取回＋临时文件 rename＋去重 |
| S3.05 | `index` | 7 projection | Ledger 旁挂索引 seq→（段，偏移）；可弃，损坏即重建 |
| S3.05 | `hot` | 7 projection | 内存热视图：界面查询在此命中，不读盘 |
| S3.05 | `projection` | 7 projection | redb 磁盘冷视图：Recycle Bin＋进度视图＋重启恢复 |
| S3.06 | `attribution` | 7 projection | 成本归因：逐维度精确分割同一总额；A20 对账 |
| S3.07 | `checkpoint` | 4 适配器 | git2 波前 add -A＋波后补记＋staged diff secret 扫描 |
| S3.07 | `queue` | 7 projection | 一份实现服务三队列；admit 先于入队，去重先于副作用 |
| S3.07 | `digest_cache` | 4 适配器 | 内容哈希→摘要 Artifact；同哈希终生一次 |

## 2 验收标准

- `JsonlLedger` 过 kernel conformance 六断言（含确定性双灌对拍）。
- proptest：任意 draft 序列落盘后，尾部任意字节级破坏（截断/追加垃圾）→ 重开＝最长合法前缀＋一条 `log_truncated`，链续可验，续写不断链。
- A16：读 `fixtures/ledger-v2/` 高版本夹具 → 方向感知拒绝（报「由更新版本写成」＋原始路径），恒不部分解读。
- A3 前两点：断电于 EventRecord 落账／CAS rename 各恢复一次（FaultFs 点阵驱动）。
- CAS：put→get 往返；范围取回（L/B 两式）；去重（同内容二次 put 不二次物化）；断电只留 `tmp/` 半成品，已命名对象恒完好。
- S3.05：同一 Ledger 两次重建，projection 逻辑导出字节逐字节相同；删库重建后导出不变（形状 7 的 proptest 骨架一次三实例化）；index 损坏即重建且查询结果不变；hot 与 projection 对同一流的重叠查询答案一致。
- S3.06：A20——各维度归因之和恒等于同期权威计费额之和（逐维度断言，整数精确无余无溢）。
- S3.07：A14 先行半链——波前 checkpoint_committed 携 oid；波后删除逐条补记 file_discarded{restoration=Tracked(波前 oid)}；含 secret shape 的 staged diff 拒提交（E_SECRET_EGRESS，恒不回显字节）；queue 去重先于副作用＋shed 不丢已入队项；digest_cache 同哈希二次 put 幂等。

## 3 假设与歧义

1. **组提交的 S1 形态**：「批＝上一次 fsync 期间到达的 append 量」这条预设并发到达；S1 单写者同步世界里，批＝一次 `append_all` 交付的入账波（并行执行、串行入账的「波」）。trait 的单条 `append` ＝单元素波。tokio 落地（S3）后按原语义重审，接口不变。
2. **断尾扫描范围**：append-only 事故只伤尾部——只完整校验最后一段，并以前一段末行验跨段链续；更早段的全链校验归 replay（A2）。
3. **目录 fsync**：POSIX 上新建/rename 后同步父目录；Windows 无目录句柄同步原语，`sync_dir` 为显式 no-op。断电点阵在 FaultFs 模型层保持严格语义（未 sync_dir 的目录项不存活），使代码纪律跨平台一致；「fsync 返回但未落盘」的平台复验属 A3 完整版（S4 测量脚本期）。
4. **存储写失败码**（S2 期初定）：`append` 的 Io 失败映射 `E_STORAGE_FATAL`（装载期第 5 码，进程级 fatal）；误名债务已清。

## 4 现状分析

空壳。热路径＝append（chain_hash＋write＋fsync）；fsync 主导，BLAKE3 与 serde 开销可忽略。

## 5 权威信源

落盘形态/断尾/版本方向/组提交/分段；CAS 三工程事实；FaultFs 三故障与断电四注入点；确定性；断电与存储边界；ARCHITECTURE.md §4（内缝 Vfs 不升真缝）与 §12 的 memory 段；kernel-SPEC §8-4/§8-9。

## 6 命名统一

Vfs、RealFs、FaultFs、FaultPlan、power cut、tail-truncation recovery（断尾恢复）、direction-aware refusal（方向感知拒绝）、segment（分段）、group commit（组提交）、CAS、dedup。crate 根错误 `MemoryError`（每 crate 一根，跨界映射 AxError 不透传）。

## 7 模块边界

```
jsonl ──声明──▶ pub(crate) trait Vfs（内缝；RealFs 同文件）
fault_fs ──实现──▶ Vfs（第二适配器；#[cfg(any(test, feature = "fault"))]）
cas ──使用──▶ jsonl::Vfs
error.rs 不设：MemoryError 住 lib 下唯一非索引宿主？——否：一模块一文件，MemoryError 与映射住 jsonl.rs（S1 唯一汇聚点不足 3 处；≥3 处跨模块汇聚时按 ARCHITECTURE §5 登记独立 error 模块）
```

**本 crate 不做什么（否定式；S3 增两条）**：
- 不裁决任何语义——kind 二分、载荷校验、规范字节全部来自 kernel；jsonl 只定 seq/prev 与介质。
- 不采样时钟（clippy disallowed 已看守）——`log_truncated` 的 `t` 由 open 的调用方注入；checkpoint 的提交时间同规入参。
- 不向调用方暴露分段——segment 边界、滚动阈值、文件名全为内部事务；对外只有目录（index 的 seq 寻址经 jsonl 的 pub(crate) 读面，不破此墙）。
- projection 族恒不成为第二历史——三视图（hot/projection/attribution）与 queue 状态全部可删可重建，恢复逻辑恒不读它们做判定。
- 不解读语义载荷之外的字段——各 projection 只消费已入账事件的声明字段，不反推、不补獼、不修复历史。

## 8 接口先行（按模块分章）

### 8-1 memory::jsonl（S1.07）

```rust
pub(crate) trait Vfs {                      // 内缝：不出对外接口，不升真缝
    fn create_dir_all(&mut self, dir: &Path) -> io::Result<()>;
    fn list(&self, dir: &Path) -> io::Result<Vec<PathBuf>>;     // 排序后返回：遍历确定性
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn append(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn truncate(&mut self, path: &Path, len: u64) -> io::Result<()>;
    fn sync_data(&mut self, path: &Path) -> io::Result<()>;
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    fn sync_dir(&mut self, dir: &Path) -> io::Result<()>;       // Windows no-op（§3-3）
    fn remove_file(&mut self, path: &Path) -> io::Result<()>;
    fn exists(&self, path: &Path) -> bool;                      // cas 去重与重开容忍需要
}
pub(crate) struct RealFs;                   // std::fs 直译，零策略

// 公开面不带泛型：Vfs 是 pub(crate) 内缝，若 JsonlLedger<V: Vfs> 公开即私有 trait 漏入公开签名（E0445）。
// 故 Box<dyn Vfs> 内藏；生产构造子 open(dir, now) 恒用 RealFs，注入点为 pub(crate) open_with（本 crate 测试）
// 与 S4 的 feature="fault" 构造子（取具体 FaultFs，trait 不出门）。
pub struct JsonlLedger { /* Box<dyn Vfs>、dir、当前段路径、段内字节数、next_seq、prev、roll_bytes */ }
pub struct OpenReport { pub recovered: Option<TailTruncation> }   // 断尾发生与否
pub struct TailTruncation { pub dropped_bytes: u64 }

impl JsonlLedger {
    /// Opens (or initializes) the ledger directory. `now` feeds the
    /// `log_truncated` record when tail recovery fires — time is a
    /// parameter here, never sampled (determinism rule 2).
    pub fn open(dir: &Path, now: TimeMs) -> Result<(Self, OpenReport), MemoryError>;
    pub(crate) fn open_with(vfs: Box<dyn Vfs>, dir: &Path, now: TimeMs) -> …;  // 测试注入点
    /// Group commit: one durability barrier for the whole wave.
    /// Ok ⇒ every line of the wave is on its segment and synced.
    pub fn append_all(&mut self, drafts: Vec<EventDraft>) -> Result<Vec<EventRef>, MemoryError>;
    pub fn read_raw_lines(&self) -> Result<Vec<Vec<u8>>, MemoryError>;   // 实例读面
    /// P1.02：写路径观察者（至多一个，后装者取代前装者）。只在**整波持久化完成后**逐条回调：
    /// 观察者因此看不到一条未落盘的事件，而推给界面的事实就是历史里的那一行。
    pub fn observe(&mut self, sink: WriteObserver);   // WriteObserver = Box<dyn FnMut(&EventRecord) + Send>
    pub fn position(&self) -> Seq;    // P1.13：现在写一条会落在哪；只给位置不给内容
}
/// 只读读面（replay/夹具）：不走 open、不触发断尾与任何写——重演恒不修盘（runtime-SPEC §8-1）。
pub fn read_raw_lines_at(dir: &Path) -> Result<Vec<Vec<u8>>, MemoryError>;
/// 目录里的账本段，按应读顺序（`list` 已排序，段名零填充故字典序即时序）。
/// 空结果的意思是「这里没有账本」，与「账本里没有事件」不是同一件事；
/// `read_raw_lines_at` 对两者都答 `Ok([])`，故需要区分的调用方问这一面。
/// 现在只有一个：`sprawling replay`，它的路径是人敲的（sprawling-SPEC §12）。
/// 段名规则因此只住 `is_segment` 一处，不被谁再拼一遍。
pub fn ledger_segments_at(dir: &Path) -> Result<Vec<PathBuf>, MemoryError>;
impl kernel::Ledger for JsonlLedger { /* append = append_all(vec![d]) */ }
#[cfg(feature = "conformance")] impl LedgerInspect for JsonlLedger { … }
// 测试可调滚动阈：roll_bytes 字段＋#[cfg(test)] 设定器；生产恒为 SEGMENT_ROLL_BYTES。

pub enum MemoryError {                      // thiserror；crate 根
    Io { op: &'static str, path: PathBuf, source: io::Error },
    VersionAhead { path: PathBuf, v: u32 }, // 方向感知拒绝的机器面
    Envelope { path: PathBuf, line: u64, source: AxError },  // open 期行解析失败＝断尾候选之外的段中损坏
    CasMissing { hash: String },
    CasCorrupt { hash: String, path: PathBuf },
    RangeOutOfBounds { hash: String },
    Draft { source: AxError },              // 组 log_truncated 草稿/规范字节失败（实践不可达，不以死变体粉饰）
}
// 创世行撑裂且仅此一段：回到空城，OpenReport 报告但账上无 log_truncated——账本身不存在，
// 且首行必须是 city_initialized；首行可解析但链根不对＝Envelope（人裁），不静默清场。
impl MemoryError { pub fn into_ax(self) -> AxError; }   // 跨 crate 边界的唯一出口
```

**落盘形态**：目录内 `ledger-<first_seq 20 位零填>.jsonl` 若干段；行＝`canonical_line`＋`\n`；链与 seq 跨段连续。滚动：当前段字节数 ≥ `SEGMENT_ROLL_BYTES` 时下一波起新段（新段创建后 `sync_dir`）。
**open 六步**：①列段排序；②空目录＝新 Ledger（next_seq=FIRST、prev=GENESIS_PREV）；③读首段首行验 `v`——高于 `EVENT_LOG_V` 即 `VersionAhead`（先于一切链检，恒不部分解读）；④校验最后一段：逐行 parse＋段内链续，首个非法字节起截断（`truncate`＋`sync_data`），跨段 prev 以前段末行验证；⑤若截掉字节>0（含「截空整段即删段文件」的退化情形），append 一条 `log_truncated`（run=CITY、who="system"、data={"dropped_bytes":n}）；⑥恢复 next_seq/prev 内存态。
**append_all 五步**：逐 draft：seq=next、`EventRecord::from_draft`、`canonical_line`、必要时滚段；写段；单次 `sync_data`（跨段波对每个触及段各一次）；更新 prev/next_seq；铸 refs。任何 Io 错误⇒整波失败，内存态不前进（下次 open 断尾清理半行）。

### 8-2 memory::fault_fs（S1.08）

```rust
#[cfg(any(test, feature = "fault"))]        // 测试与 citysim（S4 故障面）两个消费者
#[derive(Clone)]                            // 句柄共享状态（Rc<RefCell>）：断电后以同一实例重开
pub struct FaultFs { /* files: BTreeMap<路径, FileState{durable, live, durable_entry}>、op 计数、FaultPlan */ }
pub struct FaultPlan { pub cut_at_op: Option<u64>, pub cut_on_write: Option<&'static str>, pub torn_tail: TornTail }
                                            // cut_on_write（整修卡 R2.17 增）：首个字节含该串的 append 断电，一次即消费
pub enum TornTail { None, KeepBytes(u64) }  // 撕裂写：每文件未同步增量保留前 k 字节；全显式即全确定，不需种子

impl FaultFs {
    pub fn new(plan: FaultPlan) -> Self;
    /// Simulates power loss now: live falls back to durable (+ torn
    /// prefix); files whose dir entry was never synced vanish.
    pub fn power_cut(&self);
    pub fn op_count(&self) -> u64;
}
impl Vfs for FaultFs { /* 每 op 自增计数；append 先落 live 再判 cut（撕裂可咬本次写），其余 op 先判；命中即 power_cut 并报 io::Error，plan 消费后后续 op 照常（重开阶段） */ }
```

**模型三则**（比真实平台严格，故纪律跨平台成立）：①`sync_data` 前的字节不存活：durable/live 两平面，断电即 live 回落 durable，撕裂按 `TornTail` 多留未同步增量前缀；②新建文件在 `sync_dir` 前目录项不存活，断电即消失（含已 sync_data 者——比 POSIX 更严，使建段后必 sync_dir 的纪律跨平台承重）；③rename 自身原子——恒不出现半个目标文件（rename 随 S1.09 入 Vfs 与本模型）。
**第二个旋钮 `cut_on_write`，与两个入口（整修卡 R2.17）**：

```rust
impl JsonlLedger {
    #[cfg(any(test, feature = "fault"))]
    /// 收具体 FaultFs，故 Vfs 缝不出门（缝表不动，depmap 不动）
    pub fn open_faulty(fs: FaultFs, dir: &Path, now: TimeMs) -> Result<(Self, OpenReport), MemoryError>;
}
```

`open_with` 的 doc 此前写着「the `fault` feature adds a public constructor at S1.08」，而**那个构造子从未被写出来**——本文 §7 的接线图与上面那句都记着它，代码里没有。本卡把它补上：它返回的就是生产用的同一个 `JsonlLedger`，于是 crate 之外的调用方跑的是真代码，只丢掉它点名的那一次写。

**为什么按内容而不只按序号**：`cut_at_op` 在本 crate 内部好用，因为操作序就在眼前。**在装配层它是一个注定碎掉的数字**：要正好落在 `roadmap_claimed` 那一次 append 上得数一个魔术数，而上游任何一处多读一个文件就全盘失效。`cut_on_write` 让调用方用自己的词汇点名那一行——**它仍然完全显式、完全确定**（本模块自述「Everything is explicit… there is no randomness」，这条不破它），并且直接表达要问的那件事：假如这一行没落下。取 `&'static str` 是为了让 `FaultPlan` 保持 `Copy`；点名一条账本行用的是字面量。

**断电点阵初版**：以 `cut_at_op` 扫描 1..=N 全部注入点各跑一遍「写入→断电→重开→断言」；断言两条：链恒可验，**已返回 Ok 的波恒存活**（append_all 耐久契约的机器面）。S1.08 落 A3 点 1（EventRecord 落账），点 2（CAS rename）的终断言随 S1.09 cas 卡；git commit／projection 写两点随其模块（S3）接入同一点阵。

**合并拆成决定与动作（整修卡 R2.18）**：

```rust
impl Worktrees {
    pub fn plan_merge(&self, name: &WorktreeName) -> Result<PlannedMerge<'_>, MemoryError>;  // 只读；全部拒绝在此
}
pub struct PlannedMerge<'a> { /* 私有：trees、target */ }
impl PlannedMerge<'_> {
    pub fn commit(&self) -> String;               // 干线将指向的 commit，供那条行写
    pub fn apply(self) -> Result<(), MemoryError>; // 移动干线；`PlannedMerge` 无第二来源
}
// 旧 `merge` 已删（读者写者全部迁完：assembly 一处、本模块自测两处、collab/tests/pr_flow 一处）
```

原 `merge` 一次做了三件事：判快进、移干线、检出。**判定的输入在动世界之前就全部齐了**——`theirs` 就是分支尖，`ours` 就是干线尖，两者都读得到，所以「这一合并会落在哪个 commit」与「它可不可以合」都能先答。拆开之后，装配层得以按 §8-24 的规矩落行在前、动世界在后，而 §8-24 当初担心的那件事——「先落账就是把一句谎写进历史里的可达路径，因为 `merge` 有一条可达的失败臂 `MergeStale`」——**不再可达**：那条失败臂现在住在 `plan_merge` 里，早于任何一行。两头因此同时成立。

形制与 `bin::effect` 的 `Landing`／`Then` 同源：动作只能从决定里拿到，写反顺序等于去取一个取不到的值。

### 8-3 memory::cas（S1.09）

```rust
pub struct Cas { /* Box<dyn Vfs>、dir —— 非泛型，理由同 jsonl（Vfs 不得漏入公开签名） */ }
impl Cas {
    pub fn open(dir: &Path) -> Result<Self, MemoryError>;           // RealFs；建目录＋清 tmp/*.part
    pub(crate) fn open_with(vfs: Box<dyn Vfs>, dir: &Path) -> …;    // 测试注入点
    /// Content-addressed put: tmp + rename, dedup by existence.
    pub fn put(&mut self, bytes: &[u8]) -> Result<B3Hash, MemoryError>;
    pub fn contains(&self, hash: &B3Hash) -> bool;                  // 存在判定无可失败面，不包 Result
    /// Full read re-verifies the hash (cheap: BLAKE3 GB/s); mismatch ⇒ CasCorrupt.
    pub fn get(&self, hash: &B3Hash) -> Result<Vec<u8>, MemoryError>;
    /// Range read per Locator semantics (L: 1-based closed; B: 0-based closed).
    /// Trusts the object as verified at put; full-read paths re-verify.
    pub fn get_range(&self, hash: &B3Hash, range: &Range) -> Result<Vec<u8>, MemoryError>;
}
```

布局：`<dir>/b3/<hex 前 2>/<hex64>`；临时件 `<dir>/tmp/<hex64>.part`（内容定名：同内容并发写者汇合于同一目标，无随机源；残留 tmp 先 truncate 再写）。put 四步：hash→已存在即去重返回→写 tmp＋`sync_data`→`rename`＋`sync_dir`（分片目录）。范围取回越界＝`RangeOutOfBounds`（fail-closed，不静默夹取）；`L` 式行切分按 `\n`，末行无终止符同计一行；返回字节含行间 `\n`、不含末行终止符；`B` 式按 0 起闭区间直切。
rename 至此入 Vfs（jsonl 卡期未用故未入）；FaultFs 模型：rename 原子；新目标目录项在 `sync_dir` 前不存活，断电即整体消失（源已移除）——看似比真实更损，但 put 尚未返回 Ok，无可观察效果被丢失，A3 点 2 的断言面（已命名对象恒不腐蚀）不受影响。

### 8-4 memory::index（S3.05；形状 7）

```rust
pub struct LedgerIndex { /* entries: BTreeMap<Seq, (String, u64)> —— 段名＋行首字节偏移；私有 */ }
impl LedgerIndex {
    /// Build by scanning the ledger dir; 旁挂 cache `index.cache` is
    /// loaded when checksum-fresh, rebuilt otherwise (disposable by design).
    pub fn load_or_rebuild(dir: &Path) -> Result<LedgerIndex, MemoryError>;
    pub fn refresh(&mut self, dir: &Path) -> Result<(), MemoryError>;              // V3.02：只读长出来的字节
    pub fn reader(&self, dir: &Path) -> LineReader<'_>;                            // V3.01：取行的唯一入口
    pub fn persist(&self, dir: &Path) -> Result<(), MemoryError>;                  // 写 cache；失败非致命（可弃物不阻止主链路）
    pub fn len(&self) -> usize;  pub fn is_empty(&self) -> bool;                   // S3.05 增：重建后自证条数
    pub fn tail_seq(&self) -> Option<Seq>;
}
/// 一次查询期间的取行游标：持有当前段的句柄与它的逻辑位置。私有字段。
pub struct LineReader<'index> { /* index、dir、Option<段名＋BufReader<File>＋下一行偏移> */ }
impl LineReader<'_> {
    pub fn line_at(&mut self, seq: Seq) -> Result<Vec<u8>, MemoryError>;           // 取一条（顺序读零 syscall 开销）
}
```

- 旁挂 cache 格式：首行 `idx v1 <全目录字节数> <b3 前 16>`＋逐行 `seq segment offset`；校验不符／解析失败／目录字节数变化 → 静默重建（可弃即不报错）。失效判定粗粒度（全目录字节数）是刻意的：旁挂物宁可重建不可误信。
- V3.01 落地记录（**取行从「每行一次 open ＋逐字节 read」改为游标**）：一次 `History`／`RunHistory` 查询要取一段连续的 seq，而旧 `line_at` 每一行重开段文件、再一次一个字节 `read` 到换行——系统调用数与行长同阶。句柄因此住进 `LineReader`：段名不变即不重开，读用 `BufReader::read_until(b'\n')`，一次填充服务多行。
  - **实测**（5 万条账本，windows-x86_64 NVMe，2026-09-02，每种模式 200 行）：顺序读（`history`）734 → **0.89 µs／行**；逆序读（`run_history`）706 → **5.82 µs／行**；随机跳读 649 → **14.9 µs／行**。探针一次性，读完即删。
  - **位置自持**：游标记住下一行的偏移，与所求偏移相同即不 seek（顺序读全程零 seek），不同则绝对 seek 并弃缓冲。`run_history` 逆序读每行付一次 seek 与一次缓冲填充，仍是常数次系统调用。
  - **位置用 `Option<u64>` 表达，算不出来就作废**：seek 前、读前各置 `None`，只在一次成功的读之后写回 `offset + 读长`；长度换 `u64` 失败或相加溢出则继续为 `None`，下一次调用必 seek。一个可能错的位置会让游标把别的行的字节交在调用方要的 seq 名下，而旁挂物宁可重做不可误信（与 cache 失效同一条反射）。
  - **`LedgerIndex::line_at` 一并删除**，不留转发壳：读一行只此一条路，否则「怎样读一行」有两个权威，而慢的那个还在原地招手。读者全部迁完（`bin::assembly` 的 `history`／`run_history`、`tests/derived_views.rs` 的 index 实例、本模块单测）。
  - **段不因此出门**：`LineReader` 的公开面只有 `line_at(seq)`，段名与偏移仍是内部事务（§7 第三条）。
- V3.02 落地记录（**索引常驻，不再每次查询重建**）：`load_or_rebuild` 在**每一次** `History`／`RunHistory` 里把整个 `index.cache` 读进来重新解析。5 万条时是 1.5 MB 与 5 万次 String 分配，实测 **14.4 ms 一次查询**。
  - **增量面是 `refresh`，不是“追加时告知索引”**：调用方手里只有 `EventRecord`，段名与偏移是 `jsonl` 的内部事务（§7 第三条）。让观察者携偏移会把分段泄给调用方，而 `refresh` 把那个知识留在本模块。
  - 索引因此多记一张 `scanned: BTreeMap<段名, 已折入字节数>`。`refresh` 逐段比对：**变大就只读新那一段字节**；**变小或消失就全重建**（断尾修复截过段，旧偏移不再可信）；一字未动就什么也不做。
  - `load_or_rebuild` 走 cache 命中时，`scanned` 取当下段大小：**cache 新鲜的定义就是字节数与 stamp 相符**，所以这个赋值精确而不是近似。
  - 消费者：`bin::assembly` 的 `Views` 持一份，每次查询先 `refresh`。刷新代价是一次 `read_dir` 加逐段 `metadata`，与账本大小无关。
- S3.05 落地记录：`seq_of` 只探 `seq` 一个字段——索引不要求整条记录可解析，破损日志上的索引正是修复路径所需；残尾（无换行结尾）跳过不入索引，其修复归 jsonl。段名排序由本模块自持（不信文件系统枚举序）。新增 `MemoryError::SeqMissing{seq}`（→ `E_INVALID_ARGS`）：问一条从未写过的 seq 是调用者错，不是损坏。

### 8-5 memory::hot（S3.05；形状 7）

```rust
pub struct HotView { /* runs: BTreeMap<RunId, RunHot>、counts —— 私有 */ }
pub struct RunHot { pub phase: RunPhase, pub last_seq: Seq, pub last_kind: EventKind, pub who: String }
#[non_exhaustive] pub enum RunPhase { Active, Frozen }
impl HotView {
    pub fn new() -> HotView;
    pub fn apply(&mut self, record: &EventRecord) -> Result<(), MemoryError>;   // 增量；重复 seq 幂等（只前进）
    pub fn runs(&self) -> impl Iterator<Item = (&RunId, &RunHot)>;              // BTreeMap 序
    pub fn active_count(&self) -> u64;  pub fn frozen_count(&self) -> u64;
}
```

- 界面查询在此命中不读盘；run_started→Active，run_frozen→Frozen；其余事件只推进 last_seq/last_kind。S4 界面接线前唯一消费者＝citysim 与测试（台账登记）。
- **城市级记录不进 run 表**（F2.04 抳出）：`RunId::CITY`（nil）标记的是属于城而不属于任何 Run 的记录——创世记录、`building_created`。旧实现把它们折进 run 表，于是 `active_count()` 在一座**从未派过活的城**里返回 1。这个缺陷是在界面上被看见的：城市页读服务端的这个数、写「1 run in flight」，而总览页折同一条流写「什么都没在跑」——**一个问题两个答案，而错的那个是服务端的**。

### 8-6 memory::projection（S3.05；形状 7；redb）

```rust
pub struct Projection { /* db: redb::Database、last_applied: Option<Seq> —— 私有 */ }
pub struct ProjectionOpenReport { pub rebuilt: Option<ViewRebuilt> }  // 存盘视图读不开与否
pub struct ViewRebuilt { pub reason: String }                         // 删档之前，store 说了什么
impl Projection {
    /// 恒返回一个可用视图：读不开就删档重来，故不存在「打开失败于派生物」这一态。
    pub fn open(path: &Path) -> Result<(Projection, ProjectionOpenReport), MemoryError>;
    /// Idempotent by seq: records at or below last_applied are skipped.
    pub fn apply(&mut self, record: &EventRecord) -> Result<(), MemoryError>;
    /// One transaction per batch — the rebuild path (整修卡 R1.04).
    pub fn apply_all<'a>(&mut self, records: impl IntoIterator<Item = &'a EventRecord>) -> Result<(), MemoryError>;
    pub fn recycle_bin(&self) -> Result<Vec<RecycleEntry>, MemoryError>;    // file_discarded − discard_restored
    pub fn run_rows(&self) -> Result<Vec<RunRow>, MemoryError>;             // 进度视图：逐 Run 相＋完成态
    pub fn last_applied(&self) -> Option<Seq>;                              // 重启恢复：从此 seq 之后续接
    /// Canonical logical export: table-by-table, key-ordered, one line per
    /// row. Determinism is asserted on these bytes, not on redb's file
    /// (redb internals may differ run-to-run; the logical view must not).
    pub fn export_canonical(&self) -> Result<Vec<u8>, MemoryError>;
}
pub struct RecycleEntry { pub seq: Seq, pub t: TimeMs, pub paths: Vec<String>, pub restoration: String, pub restored: bool }
pub struct RunRow { pub run: String, pub started_t: TimeMs, pub frozen: Option<String> /* completion 字串形 */ }
```

- 三表：`meta`（last_applied）、`runs`、`recycle`；键全为定序编码（seq 大端字节／run 字串）。重建＝删文件重放；导出字节同（验收 §2）。崩溃安全委托 redb 事务（不入缝，只测重建）。
- P4.03 自愈：`open` 读不开存盘视图时**自己删档重开**，而不是把错误抛给调用方。理由是这条 recovery 本来就写在下一段里（`E_STORAGE_FATAL` ⇒「删文件重放」），却没有任何一处代码执行它——一条只写在文档里的 recovery 等于没有 recovery，而它偏偏是「选 redb 无妨、派生物可丢」这个论证的全部承重点。删档后 `last_applied` 自然为 `None`，调用方**不需要新接口**：它本来就得处理首次运行的 `None`，重放从头开始正是要它做的事。
  与 `index::load_or_rebuild` 同一条反射（「存疑即重建，而非报告」），但**不静默**：`ProjectionOpenReport` 照 `JsonlLedger::open` 的既有形状回报，`ViewRebuilt.reason` 留住 store 原话——删档销毁的正是这句话的唯一另一份拷贝，一个重置了却说不出为什么的视图谁也教不会。自愈只试一次：删档后第二次仍失败就照实抛错，于是「文件坏了」自己好，「盘满／无权限」照旧报告，无需辨认错误变体。
  用词：视图档是**删档重建**，不是 Discard——Discard 是产品概念（携 Restoration 的删除，glossary §4），不借给派生物的清扫。
- S3.05 载荷读取契约（本模块定义，S3.13 生产端照此写）：`file_discarded` 携 `paths: [string]`（Address 字串形）与 `restoration`（`Restoration` 的 serde 外标形，冷视图只留变体名——完整 Locator 住 Ledger，列表渲染不需要它）；`discard_restored` 携 `discard_seq: u64` 指向它撤销的那条。指不到任何 discard 的 restore **丢弃而非臆造行**（Recycle Bin 是历史不是待办队列，restored 项恒留列）。
- S3.05 落地记录：行值以 JSON 字串入表（无第二编码权威，导出即可读）；一次 apply ＝ 一个事务，`last_applied` 与行同事务落，故崩溃点恒在两态之一。新增 `MemoryError::Projection{op,detail}`（→ `E_STORAGE_FATAL`，recovery ＝「删文件重放」）：派生物的失败不像 Ledger 写失败那样必须停机。
- R1.04 批折叠：bench 夹具抓到逐条事务重建只有 ≈ 1.1k records/s（每条事件一个磁盘屏障，预算 50k/s 的 1/44）。`apply_all` 把屏障移到批上：一批一事务，折叠逻辑住 `fold_record` 唯一定义，`apply` 委派之；幂等不变（seq 门恩在批内逐条判），空批 abort 不提交，故字节不动。重建读数 ≈ 493k records/s（windows-x86_64，2026-08-22）。

### 8-7 memory::attribution（S3.06；形状 7）

```rust
pub struct Attribution { /* by_run、by_actor、by_segment、by_tool、by_skill: BTreeMap<String, UsdMicros>、
                            total: UsdMicros、pending_wave: … —— 私有 */ }
impl Attribution {
    pub fn new() -> Attribution;
    pub fn apply(&mut self, record: &EventRecord) -> Result<(), MemoryError>;
    pub fn report(&self) -> AttributionReport;
}
pub struct AttributionReport { pub total: UsdMicros, pub by_run: Vec<(String, UsdMicros)>,
    pub by_actor: Vec<(String, UsdMicros)>, pub by_segment: Vec<(String, UsdMicros)>,
    pub by_tool: Vec<(String, UsdMicros)>, pub by_skill: Vec<(String, UsdMicros)> }
```

- **五维度**（prefix 段位／SKILL／工具／子 Run／Building・Resident）；本 SPEC 期初写四维是漏，S3.06 补齐 `by_skill`。映射：`by_segment`＝prefix 段位（四段＋window 桶）；`by_skill`＝SKILL；`by_tool`＝工具；`by_run`＝子 Run；`by_actor`＝Building／Resident。
- S3 现状与待接点（不造假数据）：SKILL 机制属 P1 Library，故 `by_skill` 在 S3 恒入兵底桶 `no_skill`，取材契约先定为 `tool_result.data.skill`（字串，权重同字节数）；派生执行面属 P2 collab，故 `by_run` 在 S3 为“每 Run 自身花费”，`run_started.parent` 链的父子归并待派生落地后接。两处均不影响 A20：兵底桶仍参与求和，五维各自恒等 total。
- S3.06 落地记录：读取契约定死三条——`prompt_assembled` 携 `segments:[{slot,len}]`（两种载荷形均有此二字段）与可选 `window_bytes`（缺即不设 window 桶）；`tool_result` 携 `name` 与 `bytes`；`model_returned` 携 `billed_usd_micros`。**无权威计费额即归因零**（估算等于臆造钱，宁不报）；无权重基础即入诚实桶 `unattributed`／`no_tool`（不静默丢）。工具权重只属一波：结算即清，下一调用不继承上波。A20 除四断言外另以 256 例 proptest 钉（任意金额×权重组合均恒等）。
- 取材：`model_returned.data.billed_usd_micros`（权威计费额，S3.01 起入账）；`prompt_assembled` 逐段 len；`tool_result` 的 name。每维度独立分割同一总额：by_run/by_actor 按事件归属；by_segment 按最近一条 prompt_assembled 的段 len 最大余额法分割（四段＋window 桶：入窗历史份额）；by_tool 按前一波 tool_result 字节最大余额法（无波则 no_tool 桶）。最大余额法使每维度和恒精确＝total（A20 的整数保证）。

### 8-8 memory::checkpoint（S3.07；形状 4；git2）

```rust
pub struct Checkpoint { /* repo: git2::Repository、scope: 相对路径前缀 —— 私有 */ }
impl Checkpoint {
    pub fn open(city_root: &Path) -> Result<Checkpoint, MemoryError>;      // 无仓即 init（创世提交由首次 wave_pre 产）
    /// Commits once when the repository has no HEAD, and never otherwise.
    /// P3.02: a worktree branches from a commit, so a city that was never
    /// fenced cannot lend a tree; committing on every dispatch instead
    /// would move the trunk under every request already waiting.
    pub fn ensure_base(&mut self, scope: &str, t: TimeMs, who: &str) -> Result<Option<Payload>, MemoryError>;
    /// Pre-wave fence: add -A within scope + commit at injected time.
    /// Returns the checkpoint_committed payload {oid, files}.
    pub fn wave_pre(&mut self, scope: &str, t: TimeMs, who: &str) -> Result<Payload, MemoryError>;
    /// Post-wave sweep: deletions since pre_oid, each as a file_discarded
    /// payload with restoration=Tracked(file:<addr>@<pre_oid>).
    pub fn wave_post(&mut self, pre_oid: &str) -> Result<Vec<Payload>, MemoryError>;
    /// Staged-diff secret scan; a hit refuses the commit (E_SECRET_EGRESS,
    /// positions only, never the bytes).
    pub fn scan_staged(&mut self) -> Result<(), MemoryError>;
}
```

- S3.07 落地记录（checkpoint）：`open` 无仓即 `init` 但**不造创世提交**（空仓是合法态；在此臆造历史会使首个 checkpoint 无法归属）。暂存用 `add_all`＋`update_all` 两步（后者含删除），glob 限于 `<scope>/*`。`wave_post` 走 pre 提交树的 `TreeWalk` 比对工作区存在性，输出按路径排序（确定性）。secret 扫描在**提交之前**扫 index blob，命中即拒且只报 `path:start+len`——回显字节本身即泄漏。新增 `MemoryError::Checkpoint{op,detail}`（→ `E_WORKTREE_BUSY`，**此码由此获得首个消费者，待消解清单可划去一条**）与 `SecretEgress{locations}`（→ `E_SECRET_EGRESS`）。
- P2.03 补：`open` 逐次钉仓库局部 `core.autocrlf=false`。城里的文件必须逐字节往返，而这台机器的 git 有可能被配成在检出时重写行尾；被重写的文件与 Ledger 里它的哈希不符，而那看起来像损坏不像设置（P2.03 的 worktree 检出抳出此事）。
- 提交身份固定 `sprawling <sprawling@local>`；时间恒入参（git 签名时间＝t，确定性 2）；scope 外文件恒不入 add（WriteDomain 即边界，全树扫描被明拒）。无变化波：wave_pre 产空提交（同树 oid，仍记 payload——链可重建优于省一次提交）。

### 8-13 memory::changes（ux-14；形状 4 适配器；git2）

```rust
pub enum Lines { Counted { added: u32, removed: u32 }, Binary }
pub enum How    { Added, Modified, Deleted, Renamed { from: String } }
pub struct FileChange { pub path: String, pub how: How, pub lines: Lines }
pub enum Head   { Commit(GitOid), WorkingTree }
pub fn between(city_root: &Path, base: GitOid, head: Head)
    -> Result<Vec<FileChange>, MemoryError>;
```

**写入侧早就是 git 原生的，缺的是整个读出侧。** 每一次工具浪前 `wave_pre` 都落一个真 commit，
而仓库里没有任何一处读得出两个 commit 之间变了什么。人要的是「这个 agent 动过哪些文件」，
而那个事实已经在盘上。

**它天然只含写域。** `stage_scope` 只暂存 `<scope>/*`，所以两个检查点之间的差异不可能包含
会话只读过的文件——其他 harness 正在为这件事头痛（一个会话的 diff 把读过的文件也算进去），
而这个设计因为栅栏就是写域而白得。

**`Lines` 是穷举枚而不是两个数。** 二进制文件没有行数，把它画成 `+0 −0` 是界面在说假话；
同理 `How::Renamed` 与「删一个加一个」是两件事。

**只算数，不搬补丁文本。** `scan_staged` 拒绝回显命中的字节（「回显字节本身即泄漏」），
而补丁文本就是文件内容过 socket——同一个出口问题。路径与计数是一个查询；单文件的 hunk
得是另一个显式查询，并且必须过同一道 `kernel::secret::scan`。**本卡只做前者。**

**不缓存。** 两个 oid 都不可变，所以结果可以永久缓存（`digest_cache` 是现成先例）；
但先测再调，未测到慢之前多一张表就是多一份要同步的状态。

**一件尚未裁定的事（不阻本卡）**：`Checkpoint::commit` 写的是城仓库的 `HEAD`，而城是围绕
人已有的文件夹形成的（`web::drop` 的文档明写）。即每一次工具浪都在往人自己的分支历史里写 commit。
Conductor 与 Kilo Code 都明确拒绝了这条路，改用独立快照仓库。本卡只读不写，结论不受影响；
但**在把这些 commit 摆进界面给人看之后，这个问题会变得更难改**。

### 8-9 memory::worktree（P2.03；形状 4 适配器＋形状 2 值类型；git2）

```rust
pub struct WorktreeName(String);        // 文件系统安全；无分隔符、无点开头
pub struct Worktrees { /* repo、home、ceiling —— 私有 */ }
pub struct WorktreeLease { /* name、path、disk —— 私有 */ }
impl Worktrees {
    pub fn open(city_root: &Path) -> Result<Worktrees, MemoryError>;
    pub fn claim(&self, name: &WorktreeName) -> Result<WorktreeLease, MemoryError>;
    pub fn release(&self, lease: WorktreeLease) -> Result<(), MemoryError>;
    pub fn live(&self) -> Result<Vec<WorktreeName>, MemoryError>;
    /// P2.07：把一个节点已提交的活带进城的 trunk，返回落地的 commit。
    pub fn merge(&self, name: &WorktreeName) -> Result<String, MemoryError>;
}
impl WorktreeLease {
    pub fn name(&self) -> &WorktreeName;  pub fn path(&self) -> &Path;  pub fn disk(&self) -> ByteLen;
    pub fn opened_payload(&self) -> Result<Payload, MemoryError>;   // worktree_opened
}
```

- **一节点一棵，且它是 git worktree**：对象共享、工作树不共享，于是两个 Agent 看不见对方的中间态，而合入只走 PR 流（P2.07）。它从 `memory::checkpoint` 已在管的那个仓库分枝——城里不开第二个仓库。
- **建树前预检，不是建到一半失败**：工作树字节数 > `WORKTREE_MAX_BYTES` 即拒，拒词带当前上限与实测值。reflink 今天不尝试（无 unsafe FFI 或新依赖就没有 CoW 接口），故设计里「CoW 则 reflink，否则按上限拒」在每个平台上都只走后一臂——这是当前口径，不是已实现的 CoW。可用磁盘余量未探（std 无该接口），同样写在明处。
- **同名再领即 `E_WORKTREE_BUSY`**；能否定义掉：能，但不在本卡——当「领节点」本身变成取租约（`memory::queue` 已有队列），busy 就从错误变成排队。在那之前它是一条拒，不是一个静默的第二棵树。
- **路径不入历史**：`worktree_opened` 载荷只携 name 与字节数。绝对路径是本机事实，写进账本会使一本能搬到另一台机器的历史带上搬不走的东西。
- **merge 只走 fast-forward**（P2.07）：trunk 在节点分枝之后动过即 `MergeStale`（→`E_VERSION_CONFLICT`），不由机器把一份活重放到别人的活上面——能说出「这份活是否仍然适用」的是做它的人。拒后城内文件逐字节不变（一条断言）。
- **空仓即拒并说出原因**：worktree 从一个提交分枝，而新城在首次 checkpoint 之前没有提交；本模块恒不自建创世提交（那是 `checkpoint` 的职责，两个写入者就是两个权威）。

### 8-10 memory::queue（S3.07；形状 7）

```rust
pub struct EventQueue { /* items: BTreeMap<u64, QueueItem>、next_id、seen: BTreeSet<IdemKey>、stats —— 私有 */ }
pub struct QueueItem { pub id: u64, pub key: IdemKey, pub payload: Payload }
impl EventQueue {
    pub fn new(lane: QueueLane) -> EventQueue;
    /// Admission first (kernel::backpressure), then enqueue; Shed returns
    /// the verdict to the caller (who accounts backpressure_shed).
    pub fn enqueue(&mut self, key: IdemKey, payload: Payload, now: TimeMs) -> Result<Admission, MemoryError>;
    /// Dedup before side effects: a key already consumed is Duplicate and
    /// must not reach the consumer twice.
    pub fn consume(&mut self) -> Option<QueueItem>;
    pub fn stats(&self) -> QueueStats;   pub fn len(&self) -> u64;   pub fn is_empty(&self) -> bool;
}
#[non_exhaustive] pub enum QueueLane { Signal, Approval, Repair }   // 一份实现三队列
```

- S3.07 落地记录（queue）：容量入构造子（`new(lane, capacity)`）而非写死常量——三 lane 容量不同是装配事。**重复键返回 `Admit` 而非 `Shed`**：发送方已尽职，告知失败只会招致无效重试；`seen` 持久于队列寿命（消费后仍认得出重复，因为副作用已跑过一次）。被 shed 项不入 `seen`，故重试不算重复。
- 重建性：队列状态＝（signal_enqueued − signal_consumed）的 projection；持久性不在本模块（Ledger 已是历史）。lane 只定账目名字段，三队列零分支差异——差异出现之日即分模块之日（反推式合并的退出条件写在明处）。

### 8-11 memory::digest_cache（S3.07；形状 4）

```rust
pub struct DigestCache { /* dir —— 私有；文件名＝内容哈希 hex64.json */ }
impl DigestCache {
    pub fn open(dir: &Path) -> Result<DigestCache, MemoryError>;
    /// Same content hash digests once for life: a second put with the same
    /// hash is a no-op returning the stored artifact.
    pub fn put(&mut self, content: &B3Hash, tree_json: &[u8]) -> Result<(), MemoryError>;
    pub fn get(&self, content: &B3Hash) -> Result<Option<Vec<u8>>, MemoryError>;
    /// Invalidation produces the digest_invalidated payload; the entry is
    /// removed so the next digest re-runs.
    pub fn invalidate(&mut self, content: &B3Hash, reason: &str) -> Result<Payload, MemoryError>;
}
```

- 消费者 runtime::digest 属 P1；本期交存储面（台账：计划内待接 P1）。写入经 tmp＋rename（复用 cas 的 Vfs 纪律）。

### 8-12 memory::bundle（P1.14；形状 4 适配器＋形状 2 值类型）

```rust
pub struct Manifest { /* 私有；records、head、cas_objects、files */ }
impl Manifest {
    pub fn records(&self) -> u64;         pub fn head(&self) -> &str;   // 链头哈希
    pub fn cas_objects(&self) -> u64;     pub fn files(&self) -> u64;
}
pub struct Bundle;
impl Bundle {
    pub fn export(city_root: &Path, dest: &Path) -> Result<Manifest, MemoryError>;
    pub fn restore(bundle: &Path, city_root: &Path) -> Result<Manifest, MemoryError>;
    pub fn read_manifest(bundle: &Path) -> Result<Manifest, MemoryError>;
}
pub const MANIFEST: &str = "MANIFEST.json";
pub fn open_restored(city_root: &Path, now: TimeMs) -> Result<PathBuf, MemoryError>;  // 恢复后可继续写
// MemoryError 增一臂：Bundle { op, detail }——I/O 正常但不是一座城（目的地已占、清单对不上、链有缺口）
// Vfs 内缝增 `list_dirs`：两个适配器同改；list 与 list_dirs 都是浅层，遍树用显式工作表（不递归，栈溢出接不住）
```

- **带走什么**：`ledger/`（唯一历史，必带）、`cas/`（Locator 指进去，不带就断链）、城里的产品文件（`City.md`、各楼的 `BUILDING.md`／`Roadmap.md`／`URBANITE.md` 与房间内容）。**不带**：projection 与索引（可弃，恢复后由 Ledger 重建，带了就是第二份历史）；凭证（**它从不在城里**，在宙主机金库——导出一份能拷走凭证的备份会把隐私保证一次性作废）。
- **为何是目录而非单文件**：单文件要么自造容器格式（多一个要养的格式），要么引 tar／zip 依赖。目录两者都不要，且任何备份工具都能再打包一层——压缩不是本模块的职责。
- **清单是完整性的判据**：`MANIFEST.json` 记下记录数、链头哈希、CAS 对象数与文件数；`restore` 恢复后重算并比对。不对即拒，而不是“恢复了但少了几条”——后者是历史失真。
- **链验在 restore 内**：恢复完即走一遍 Ledger 开启与链校（jsonl 已有的那一道）。交给调用方去验等于把一个必须成立的性质变成约定。
- **文件顺序确定**：遍历走 `Vfs::list`（已排序），清单用 BTreeMap；同一座城导两次，`MANIFEST.json` 逐字节相同。
- S3.07 落地记录（digest_cache）：红转绿抳出一个真缺陷——`Vfs::append` 是**追加**，残留 `.part` 未清即发布出「残骸＋新内容」的拼接体。修法取 cas 既有两层纪律（open 清扫 tmp 残骸＋put 前 `truncate(0)`）而非另立新机制。`invalidate` 对不存在项不报错（末态即调用者所求），载荷携 `existed` 实报。

## 8.5 两个设计

**A（选中）：Vfs 内缝＋FaultFs 注入**——故障面在文件系统语义层注入，jsonl/cas 的产品代码零测试钩子。杠杆：一套故障模型服务两模块＋日后 checkpoint/projection；断电点阵是 Vfs 语义的性质，不是某模块的分支。
**B（落选）：jsonl 内置故障开关**（`#[cfg(test)]` 的注入点散布各写步）——不需要 Vfs 抽象，但故障语义与产品逻辑同居一文件，点阵无法复用给 cas，且「测试钩子进产品代码」违背「测试与产品走同一道门」。落选理由：内缝的存在证明是第二适配器，不是一句声明。

## 9 工作流程

装配（S1 期＝测试与 citysim）：构造 Vfs → `JsonlLedger::open`（断尾自愈）→ 作为 `&mut dyn kernel::Ledger` 交记录方 → 波到即 `append_all`。CAS 旁路：offload/attach 字节 `put`，Locator 携 hash 跨会话，`get/get_range` 取回。

## 10 实现逻辑

1. 行终止符恒 `\n`（含末行）；chain_hash 对不含 `\n` 的行字节计算（kernel-SPEC §8-9）。`.gitattributes * -text` 已保夹具字节。
2. open 的段校验用 `EventRecord::parse_line`＋`chain_hash` 复算，无独立解析器（一个权威）。
3. 段内偏移不建索引（S3 memory::index 的事）；`read_raw_lines` 全量读，S1 消费者只有 replay/夹具/conformance。
4. `list` 排序返回＋段名零填宽度 20：字典序＝数值序，跨平台遍历确定。
5. `truncate` 后同步；整段截空直接 `remove_file`，重开容忍残留空段文件（崩溃窗口的两态都可解析，比 rename 舞步少一个中间态）。另：最后一段首行即损坏且仅此一段时，截至 0 字节＝回到新 Ledger（append 未曾返回 Ok 即无可观察效果）；非尾段损坏才是 Envelope 错误。
6. FaultFs 的 io::Error 用 `ErrorKind::Other`＋自述文本；jsonl/cas 对错误只透传包裹为 `Io{op,path}`，不吞不换。

## 11 边界枚举

空目录首开（新 Ledger）；末段恰好整段损坏（截空删段、退至前段）；首段首行即损坏（Envelope 错误——创世行不可断尾，宁停不脏）；波跨滚动边界（两段各一次 sync）；`append_all(vec![])`（no-op，Ok(空)）；同内容并发 put（tmp 同名幂等）；get_range 恰触界（`B` 端点＝len-1 合法）；`L` 范围起于超出总行数（越界拒）；v 低于当前（v<1 不存在，按 Envelope 拒）；夹具目录只读（A16 只读不写回）。

## 12 错误处理（逐码答「能否定义掉」）

- `VersionAhead`→`E_LOG_VERSION_UNSUPPORTED`：不可定义掉——二进制升级与数据寿命天然错位；方向感知拒绝即其最小语义。
- `CasCorrupt`→`E_CAS_CORRUPT`：不可定义掉——位腐烂与外部改动在本设计边界外；能定义掉的部分（写路径半成品）已由 tmp+rename 定义掉。
- `CasMissing`→`E_PATH_NOT_FOUND`：不可定义掉——Locator 是跨会话引用，对象可被更早的介质事故清除；nearby 给同前缀既存对象。
- `RangeOutOfBounds`→`E_INVALID_ARGS`：可部分定义掉——`Range` 构造已保 `from<=to`；对象长度只在读时可知，读时校验是剩余的不可消部分。
- `Io`→`E_STORAGE_FATAL`（宁停不脏路径；不可定义掉——介质失败在设计边界外；S2 期初 verdict 增码后改正）。
- `WorktreeBusy`→`E_WORKTREE_BUSY`（P2.03）：可定义掉但不在本卡——当「领节点」本身变成取租约（`memory::queue` 已有队列），busy 就从错误变成排队。它同时承担「该节点的树被占」与「再开一棵就越上限」两个情形：两者的可执行替代同为「先归还一棵」，而区分它们的是 subject 不是码。
- `MergeStale`→`E_VERSION_CONFLICT`（P2.07）：不可定义掉——两个节点同时开工就会有一个后到；能定义掉的那部分（“合到一半失败”）已由 fast-forward 判定在动手之前定义掉。
- `Worktree`→`E_STORAGE_FATAL`（P2.03）：不可定义掉——仓库与文件系统是外部世界；能定义掉的那部分（名字走出目录）已由 `WorktreeName` 在构造点定义掉。
- `Envelope`→`E_LOG_VERSION_UNSUPPORTED` 同族拒读（段中损坏非尾部＝不可自动修复，指出路径请人裁）。

## 13 依赖选型

kernel（workspace 内层）；`thiserror`；`blake3`（经 kernel 的 chain_hash／cas 自身 hash——直接依赖，B.7 钉版）；`serde_json`（envelope 探查）。dev：`proptest`、`tempfile`（RealFs 测试隔离目录）。不引 walkdir（Vfs::list 一层足矣）。
S3.05 实测：redb 4.2.0 在钉版 1.97.1 上直接编译通过，接口取 `ReadableDatabase`／`ReadableTable`／`TableDefinition` 三件；形状 7 的重建骨架住 `tests/derived_views.rs`（`tests/` 不受 modmap 辖，与 kernel/runtime 既有集成测试同例），一次定义三次实例化（index／hot／projection），另加冷热重叠查询一致断言。
S3 增：`redb = "4.2"`（projection 冷视图；2026-08 复核：4.2.0 现行，文件格式声明稳定；MSRV 1.89 < 钉版 1.97）；`git2 = "0.21"`（checkpoint；2026-08 复核：0.21.0 现行，携 libgit2 1.9.6 vendored；libgit2 链接例外已在 `deny.toml` 登记）。两者均不入缝：崩溃安全委托其事务，只测重建与孤儿清扫。

## 14 硬编码声明

`SEGMENT_ROLL_BYTES = 64 MiB`（内部事务，非 consts_policy——对上层不可见，改它不改任何行为语义，只改文件切法）；段名前缀 `ledger-`＋20 位零填；CAS 分片取 hex 前 2；tmp 后缀 `.part`。均为 pub(crate) 常量，改动随本 SPEC。

## 15 影响面

runtime::replay 读 `read_raw_lines`；citysim 夹具对拍与断电点阵消费 FaultFs；S3 index/projection 挂同一目录布局。trait 边界 Io 映射已正名（Io→E_STORAGE_FATAL，S2 期初），无待清债务。

## 16 测试与约束

单测：open 六步各分支；滚动边界；append_all 原子性（注入 Io 后内存态不前进）；cas put/get/get_range/dedup。proptest：链续与断尾（任意截断点/垃圾尾）；FaultFs 点阵（cut_at_op 扫描）。夹具：A16 高版本拒读。conformance：JsonlLedger 过 kernel 六断言。约束：clippy 零告警；fault_fs 在非 test/fault 构建中零字节。

## 17 模型体验

零字节：本 crate 恒不进 prefix；模型可见面只有经 tool_result 携带的 AxError（如 E_CAS_CORRUPT 的 three-part 拒绝），其余全部是落盘内部事务。

## 18 文档同步

ARCHITECTURE §6 memory 表：jsonl/cas 状态翻转＋fault_fs 新行登记（S1.08 同 PR 表先行）；kernel-SPEC §12 存储码缺口共享一个 verdict；S3 模块落地时本文增章。
