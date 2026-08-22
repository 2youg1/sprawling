# sprawling-SPEC.md — bin（main＋assembly＋嵌入链）

> crate：`sprawling`（唯一 bin）。本章只覆盖 Stage 0 范围；子命令随各期扩章（init/serve/resume/replay/fork/status 的全量语义随各期在本 SPEC 落章）。

## 1 需求拆解

S0 三件：①CLI 壳（`status` 可用；未到期的子命令给出诚实拒绝而非 stub）；②`assembly` 骨架（Main＝唯一知情点的形状先立，S1 起逐缝接线）；③`build.rs` 嵌入链（web 产物 → OUT_DIR → `include_bytes!`，S0 用占位页验证链本身）。

## 2 验收标准

`sprawling status` 打印版本、嵌入字节数、已接适配器数并退出 0；未知子命令退出 2 且指明「哪些随期解锁」；单测证明嵌入字节非空且含标记（卡 S0.09 收口）。

## 3 假设与歧义

「验证这条链先于验证页面内容」——S0 不起 HTTP 服务，HTTP 属 channels::server（S4）；嵌入的取证面是测试与 `status` 输出。

## 4 现状分析

空壳。无。

## 5 权威信源

bin 子命令面；装配层是 Main；ARCHITECTURE.md §2（客户端嵌入链）与 §12（模块图的 bin 段）。

## 6 命名统一

Assembly／assemble；WEB_INDEX（嵌入资产常量）。

## 7 模块边界

`main`：CLI 分发与呈现；`assembly`：唯一知情点，句柄/时钟/种子/spawn 注入处。
**本 crate 不做什么（否定式三条）**：不含任何判定（判定住 kernel）；不直接落盘（落盘住 memory）；S0 不起网络服务（channels S4）。

## 8 接口先行

```rust
pub(crate) struct Assembly { /* 适配器随缝落地逐字段进驻 */ }
pub(crate) fn assemble() -> Assembly;   // 全库唯一的时钟采样与 spawn 授权点（届时以 #[expect] 标注）
```

## 8-2 派活回路（P1.02）

```rust
pub(crate) struct RunWorker { city_root, ledger: JsonlLedger, cas: Cas, model: Box<dyn Model + Send> }
impl RunWorker {
    pub(crate) fn new(city_root: &Path, model: Box<dyn Model + Send>) -> Result<Self, AxError>;
    pub(crate) fn observe(&mut self, sink: memory::WriteObserver);
    pub(crate) fn handle(&mut self, command: channels::WireCommand) -> Result<(), AxError>;
}
pub(crate) fn configured_model() -> Box<dyn Model + Send>;
pub(crate) async fn serve(city_root, addr, token, index_html, model) -> Result<(), AxError>;
```

- **单写者，且它不跨线程**：`RunWorker` 在自己的线程里 `new`，JsonlLedger 因此**从未跨过线程边界**——不需要为了一个 `Send` 约束去改 memory 的内缝（改了就会造成 `FaultFs` 的 `Rc` 不合法，而故障适配器本来不需要跨线程）。启动错误经一个 `sync_channel(0)` 回报，所以「开不了账」仍然是 `serve` 的错而不是一条日志。
- **命令受理与命令执行分开**：socket 任务只 `mpsc::Sender::send`，回合循环在工作线程。刷新页面不会杀掉工作，而进展从事件流回流——「关掉界面再打开」与「从未关过」在服务端看来因此无差别。
- **事件流的源头是 Ledger 本人**：`JsonlLedger::observe` 在**持久化之后**逐条回调，回调把记录扔进 broadcast。服务端因此推不出一条历史里没有的事实。
- **无 provider 时仍然能起服务**：`UnconfiguredModel` 把「没配模型」变成一条三段式拒绝，而不是拒绝启动。一座城在没有推理服务时仍然可读（重放、浏览）。配置面：`SPRAWLING_MODEL_URL`（回环）＋`SPRAWLING_MODEL`。
- **工具名录与工具台同源**：`Catalog::admit_tool` 产 `ToolDef`（模型看到的），`ToolBench::register` 负责路由（实际跑的），一次登记喂两边——否则“模型以为存在的工具”与“真能跑的工具”会成为两份名单。本期只挂 `edit` 与 `status`：`exec` 需要一个真 sandbox 与 Python WASI 配置，随出网与 sandbox 那张卡一并接。
- **RunId 是推导而非抽取**：`b3(job|addr|now)` 前 16 字节。同一毫秒对同一地址派同一件活就是同一个 Run，且标识符里不进随机数（确定性第 7 条的同一条理由）。
- **预算不可缺**：`DISPATCH_TURN_BUDGET = 24`。调用方还不能设它，但无上限地向付费 provider 循环是唯一没有天花板的失败模式。
- **`init` 写 `City.md` 入城**：二进制携默认本，城里那份是用户可改的权威；每次组装 prefix 读城里的那一份，代码里恒不长第二份副本。

## 8-2b CLI 补齐（整修卡 R1.03）

- **`resume <city>`**＝启动扫描：验链 → `dangling_tool_calls` 逐个补记 `E_TOOL_OUTCOME_UNKNOWN` 的 `tool_result`（幂等：已闭的账不重补）→ 报待批数。`runtime::replay` 补写面自此有生产消费者（P3.03 留下的台账行翻过）。续跑已批准的活仍在 serve 的 `answer_approval` 路径上，两者不重叠。
- **`fork <city> <run> <seq> [addr]`**＝世系记录形：验链 → `runtime::fork::prefix` 验界 → 节点归属验（seq 处的事件必属于母 Run，否则拒）→ `run_forked` 落账携新 RunId。**不自动发车**：驱动新 Run 是人的下一步 Dispatch；逐字节母前缀入窗属并发期的 must-read 网，不在本形。`Command::Fork` 同路。
- **`adopt <city> <addr>`**＝收编已存在目录为楼（语义住 `city-SPEC.md` §8-3）。
- **`serve` 增 `--web-dir <dir>`**：开发回路逐请求读盘；发布形恒走嵌入表（`channels::ClientAssets`，语义住 channels-SPEC §8-2）。

## 8-3 视图与 Spine（P1.03／P1.04）

```rust
pub(crate) struct Views { city_root, hot: HotView, attribution: Attribution, approvals: BTreeMap<String, ApprovalSummary> }
impl Views { fn apply(&mut self, &EventRecord) -> Result<(), AxError>; fn answer(&self, &Query) -> Answer; }
pub(crate) fn rebuild_views(ledger_dir: &Path) -> Result<Views, AxError>;   // 启动时冷重建
fn read_spine(city_root: &Path) -> Vec<BuildingProgress>;                    // 查询时读盘
```

- **视图冷重建与热折叠共用 `apply`**：启动时把 Ledger 逐行喂进去，其后由 `JsonlLedger::observe` 喂。测试断言两条路径答案逐字段相等——这就是「projection 可弃」的可执行形式。
- **Roadmap 查询时读盘，不入投影**：那份文件**就是**计划，Agent 用 edit 工具改它。把它复制进投影就是为同一件事立第二个说法，而漂开的总是没人看的那个。
- **读不懂的行照显**：`problems` 随答案回到界面；没有 Roadmap 的楼答 `Progress::Unplanned`（它没有 ratio 方法，故界面画不出百分比不是守规矩，是无从下手）。
- **保留前缀不是楼**：`.` 开头的目录跳过，`.sprawling/` 因此恒不被当成一栋楼。

## 8-4 MCP 接线（整修卡 R1.13）

```rust
// bin::mcp_stdio（形状 4 适配器；实现 protocol::Outbound）
pub(crate) struct StdioServer { /* 私有：Rc<RefCell<Inner>>，克隆即同一个子进程的第二个句柄 */ }
impl StdioServer {
    pub(crate) fn start(command: &str, args: &[String], cwd: &Path) -> Result<StdioServer, AxError>;
}
impl protocol::Outbound for StdioServer {
    fn call(&mut self, line: &str, patience: TimeoutMs) -> Result<String, AxError>;
}

// bin::assembly
fn mcp_tools(&mut self, config: &FrozenConfig, addr: &Address, confidential: bool)
    -> Vec<protocol::McpTool>;   // 起不来的 server 缺席并留下诊断，恒不拒整次 dispatch
```

- **一台 server 一个子进程，一次 dispatch 一条命**：工具表随 Run 冻结，子进程的寿命因此就是 Run 的寿命。最后一个 `McpTool` 落地时 `Drop` 杀子进程，于是「谁来回收」不需要第二份名单。
- **读取线程是句柄的一部分，spawn 点仍在 bin**（确定性七条③的口径：并发归装配层）。同步读一根管道没有期限，而一个不回答的 server 会把整个 Run 挂死——本期已经吃过一次这个形（真机 provider 在 `model_called` 之后五分钟无返回）。故 `start` 起一条只读 stdout 的线程，`call` 用 `recv_timeout` 等它；超时即杀子进程并三段式拒。**线程恒不泄漏**：杀子进程关掉管道，读到 EOF 即结束。
- **期限从声明里来，不在适配器里另写一个数**：`ToolMeta.timeout`（`tools_from` 写的 `TimeoutMs(60_000)`）既是对模型的承诺，就应当是真正被执行的那一个；否则该字段只是装饰。故期限随 `Outbound::call` 入参。
- **起不来的 server 缺席而不拒 dispatch**：与 `city::library` 对「楼里点名却不在架上的 SKILL」同形——模型看到的名录恒等于真能跑的工具表，缺席的那一件在诊断里留名。一个外部服务今天起不来，不是这栋楼今天不能干活。
- **confidential 楼：一条规则两层后果，不是两份判定**。工具能不能存在归 `protocol::McpTool::new`（构造点拒，恒是权威）；**进程该不该被拉起归装配层**，因为进程寿命本来就是这一层的职责，而一台 MCP server 可能在启动那一刻就出网。故 confidential 楼在拉起任何子进程之前就跳过整张 `[[mcp]]` 表并留一条诊断；工具层的拒仍在，它是那一层失守时的兵底。两层同向，因此不会出现「只改一处」的漂移。
- **`Effect::Connector` 是本卡推出来的 kernel 变更**（语义住 kernel-SPEC §8-23）：接线前 `tools_from` 写的是 `Effect::Egress`，而出站门从 **调用参数**里读 `host`——外部工具的参数表由 server 的 `inputSchema` 决定，里面恒没有 `host`。第一次真调用当场拿到 `E_INVALID_ARGS: declares Egress but named no host`：这就是「一个适配器是假想缝」的同一条道理在工具面上的实例——没有调用方的声明从未被那道门验过。
- **discover 先于 list，但今天不据它分支**：它当下的作用是在把任何工具交给模型之前，先证明对侧真的会应答；版本协商要有第二个版本才成立，而字段名本库今天无法从一台真的 server 上核对。读到什么写进诊断，不写进判定。
- **口径不变的那两件**：外部工具与 L0 工具同落 `kernel::tool` 缝，故结果恒自动进污染环，装配层无解包面；调用由 `ToolBench` 路由，故 `tool_called`／`tool_result` 两行自动落 Ledger，不为它另写一条入账路径。

## 8-4b MCP 的第二条传输（整修卡 R1.17）

`bin::mcp_http` 是 `Outbound` 缝的第三个适配器（stdio 子进程、ScriptedOutbound、HTTP），也是这条缝第一次真正被两条生产路径共用。`McpServer.transport` 从两个裸字段改成穷尽枚举 `McpTransport { Stdio{command,args}, Http{url,header} }`：**一行既写 command 又写 url 就是一行要读者去猜的配置**，故配置层当场三段式拒。装配层把差异全部花在 `McpLink` 这一个枚举里，其上的接线仍是一条路。

三条口径：①**HTTP 无会话**，故克隆只共享地址——这与当前修订版删掉协议级 session 同向；②**事件流只取第一条 `data:`**，读不出就拒，恒不把两条答案拼成一条工具结果；③**拒词不引用对侧正文**（服务端的错误页是别人写的字），只说状态码与该查什么。

## 8-5 订阅登录接线（整修卡 R1.14）

```rust
fn login(&mut self, provider: &str, step: channels::LoginStep) -> Result<(), AxError>;
fn login_with(&mut self, profile: &gateway::OauthProfile, provider: &str,
              step: channels::LoginStep) -> Result<(), AxError>;   // 查表之外的全部
fn random_token(bytes: usize) -> Result<String, AxError>;          // OS 熵，非种子 RNG
fn dialect_of(provider: &str) -> Result<DialectKind, AxError>;     // 已知 provider 才有答案
```

- **熵不走种子**：`random_token` 用 `getrandom`（OS 熵），**恒不**用装配持有的仿真种子——一个第三方能预测的 verifier 就是一个第三方能完成的登录。这是全二进制里唯一一处「可复现即缺陷」的地方，故写在这里而不是留给读者推断。
- **pending 只活在进程里**：PKCE 的 verifier 证明「来兑的就是当初请求的那个进程」，一个活过进程的 verifier 什么也不证明。重启＝重新开始登录，代价是一次浏览器访问。
- **`login_with` 是查表之外的全部**：生产路径先查 `oauth_profiles` 再进它；测试把一台自己控制的 server 当 profile 传进去。**恒不为测试在生产路径上加环境变量开关**——那个开关在生产里没有人会设，却永远在那里可被设。
- **登录完即 attach**：人是为了用它才登录的，故 `api_base` 一到手就接上端点，而不是留下第二件要记得做的事。`api_base` 为空的 provider 在这里三段式拒，且拒词说明令牌已在保管库里——已经发生的事恒要说出来。
- **未做且已知**：令牌续期。`expires_in` 与 refresh token 都已入库，但到期前自动换新还没有接线；在它到来之前，过期就是重新登录一次。写成明账而不是留给用户去撞。

## 8-6 五个视图不再答 unavailable（整修卡 R1.16）

`Views` 增四份折叠与一次读盘，`answer` 的 catch-all 臂随之消失——**穷尽 match 是本卡的验收之一**：此后新增一个 Query 不写答案就编译不过，而不是在运行时答一句 `Unavailable`。

| 查询 | 出处 | 口径 |
|---|---|---|
| `InboxView` | 折 `signal_enqueued` 减 `signal_consumed` | **看队列不靠消费**：`Inbox::pull` 要拿走才给得出内容，一个看一眼就把东西取走的视图会改变它所报告的对象 |
| `DiscardView` | 折 `file_discarded`／`discard_restored`，按路径归键 | 每行自带回去的路（`restoration`）；还原是**关掉它开的那一行**，不是另开一行 |
| `RegistryView` | 折 `asset_archived` | 「这座城认定值得留下的东西」；空表就是空表，与「本版本答不了」在类型上已经不可混淆 |
| `ArchiveSearch` | 被问的那一刻读盘（同 `BuildingView`） | 文件是权威，另存索引就是第二个权威 |
| `Metrics` | 上面几份＋`hot`＋`read_spine` | **恒不携钱**：钱是 `CostView` 的，一个数字两个主人就是两个数字开始互相矛盾的起点。这里每个数都已被别的视图证明过，它存在只为让画一条读数花一次问答；唯一自有的数是 `events`（本视图折过多少条），因为没有别的答案能推出它 |

## 8-7 ACP 入站与令牌续期（整修卡 R1.18）

```rust
fn acp_dispatch(desk: &CommandDesk, body: channels::AcpBody, authentic: bool)
    -> Result<channels::AcpProgress, AxError>;          // 外来请求 → 普通 Dispatch
fn renew_if_stale(&mut self, provider: &str) -> Result<(), AxError>;   // 用之前先换，不等 401
```

- **令牌在门那侧判，判定在协议那侧措辞**：`channels` 持配对令牌，故常数时间比对住 `/acp` 路由；`authentic` 这一位传进来，由 `protocol::admit` 说拒词——未配对者只学到一位，这句话的权威只有一个。
- **入站不是第二个 control surface**：admit 之后就是人按派活条时走的同一条路（同一个 `CommandDesk`、同一个 `Command::Dispatch`）。回给编辑器的只有 progress 三字段，且 run id 是工人接单时才铸的，故此刻诚实的答案是「已受理、尚未完成」。
- **续期在用之前做，不在 401 之后做**：一次 401 要花掉一整个回合才发现，而 provider 说过的到期时刻这座城已经写下来了（`secret_captured` 携 `expires_at`，非密文）。留一分钟余量；**没有记过到期时刻的 provider 不碰**——不知道什么时候过期，不是每次都换一遍的理由。
- **换新与兑付共用一次发送**：`send_token_request` 是两种 grant 的同一条路，故「不引用对侧正文」这条只写一次、也只可能对一次。

## 8.5 两个设计

**A（选中）**：`build.rs` 拷贝资产入 OUT_DIR＋`include_bytes!`——单点嵌入，S4 换 wasm 产物时只改拷贝源。**B（落选）**：`include_bytes!` 直指 `../web/assets`——少一步拷贝，但把「产物在哪」写死进源码路径，S4 换源即改代码；且无 `rerun-if-changed` 粒度。翻案条件：无。

## 9 工作流程

`main` → 解析 argv → `status`：`assemble()` → 打印三行 → 退出码。

## 10 实现逻辑

零依赖（clap 待 S1 真子命令出现时引入——现在只有一个子命令，一个 match 不值一个依赖）；`cargo::error=` 使 build.rs 失败显性（cargo ≥1.84 语法）。

## 11 边界枚举

资产文件缺失（build 期即红，不是运行期惊喜）；OUT_DIR 缺失（同上）；无参调用（用法＋退出 2）。

## 12 错误处理

build.rs 内 `Result<(), String>` 汇到 `cargo::error`；运行期无可失败路径（S0）。逐码消解：无新增码。

## 13 依赖选型

零运行时依赖（见 10）。

## 14 硬编码声明

资产相对路径 `../web/assets/index.html`（S4 随构建管线改为 wasm 产物目录，改点唯一在 build.rs）。

## 15 影响面

justfile／CI 无涉；S4 前端框架结论书将改写 build.rs 拷贝源与 `just build-web`。

## 16 测试与约束

单测：嵌入字节非空且含 `sprawling` 标记。约束：workspace lints 全量适用（含 build.rs）。

## 17 模型体验

零字节：bin 不产生任何入窗内容。

## 18 文档同步

子命令每扩一个：本 SPEC 增章、ARCHITECTURE.md §12 状态翻转、CLI 三栏表核对。
