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

三条口径：①**事件流只取第一条 `data:`**，读不出就拒，恒不把两条答案拼成一条工具结果；②**拒词不引用对侧正文**（服务端的错误页是别人写的字），只说状态码与该查什么；③当时写的「**HTTP 无会话**，故克隆只共享地址」**已于 P5.01 推翻**，见下。

## 8-4c 会话、凭据与报错地址（P5.01）

三处修正，三处都是真机跑出来的。

- **`HttpServer` 持会话，且克隆共享它**。旧口径「HTTP 无会话」读错了规范（详 `protocol-SPEC.md` §8-3）。一台 server 就是一个会话，不论一次 Run 拿了它几件工具；两个克隆各发一个 session id 就是与同一台 server 开两场对话，而它只开过一场。协商版本从**携 `result.protocolVersion` 的那一条答案**学得——按生命周期，那就是 `initialize` 的答案，因为它是一条连接的第一句话，没有更早的答案能持有该字段。
- **`header` 兑付 `secret:` 引用**。`redeem_header` 把 `Name: secret:realm/name` 在最后一刻换成真值，与 endpoint 凭据同一条路。不这么做，一把付费档的 key 会明文躺在楼里的 `CONFIG.toml`，而 `xtask secret` 看不见它（城市配置不在仓库内）。不是引用的值原样通过：头里是个账号名或固定标记的 server 无物可兑。
- **报错地址指向真正出事的传输**。`transport_site()` 按 `McpTransport` 分派；此前所有 MCP 失败都挂在 `bin::mcp_stdio` 名下，上一个跟着它去查的人被引到了错的文件。成功时同样留一行：对侧叫什么、说哪个版本、给了几件工具。

**真机验收**：一座真城接一台托管 server，诊断行为 `exa is exa-search-server speaking 2025-06-18, offering 2 tool(s)`；模型自主调用其搜索工具、读回真实结果、一个回合内给出答案并 `run_frozen{completion: done}`。

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

## 8-8 首次运行与交付形态（P7.01／P7.02／P7.03）

**病灶**：release 里的 exe 是控制台程序。无参启动只向 stderr 写一行用法并退 2，从资源管理器双击即闪退——没有安装过程，也没有任何成败提示。从 exe 到 WebUI 之间还压着 `init`／`serve`／自行输入地址三步手工操作，而 `serve` 打印的是裸 socket 地址不是 URL。终端、双击、脚本三类到达方式被挤在同一个入口上。

**不猜启动方式**：判断「我是被双击的还是在终端里跑的」，可靠办法是 `GetConsoleProcessList`，需 `unsafe`——workspace lints 恒禁。故以**显式入口**取代探测：三扇门各自命名，背后共用同一段序列。

```rust
// bin::firstrun
pub(crate) enum FirstScreen { Start(PathBuf), Quit }

pub(crate) fn default_city(exe_dir: &Path, home: Option<&Path>, exe_dir_writable: bool) -> PathBuf;
pub(crate) fn is_writable(dir: &Path) -> bool;
pub(crate) fn ask<R: BufRead, W: Write>(city: &Path, input: &mut R, out: &mut W) -> io::Result<FirstScreen>;
pub(crate) fn open_in_browser(url: &str) -> io::Result<()>;
pub(crate) fn local_url(bind: SocketAddr) -> String;
```

- **`up <dir>`＝序列的唯一定义**：目录里没有 ledger 就先 `init`，随后 `serve`，随后开浏览器。无参屏与 `start.cmd` 都落到它，`init`／`serve` 仍各自独立可用——一段序列一处权威。
- **genesis 要人同意**：写 Ledger 第 0 行是全系统唯一一次不可撤销的语义写入，不因「有人双击了一个文件」而发生。无参屏在按键**之前**把最终路径显示出来，人按回车才开城；`q` 退出并打印命令表。
- **非交互 stdin 无此问**：`read_line` 得 EOF（管道、CI、无人值守）即 `Quit`，主流程打印命令表退 2。这条让该路径在没有 TTY 的地方也可测。
- **默认位置取 exe 同级 `city/`**：整座城随文件夹可拷、可备、可删，与「一座城市就是一个目录」同构。`is_writable` 探到不可写（解压进 Program Files）就回退 `home/sprawling/city`；回退可见而非暗中，因为路径印在第一屏上。
- **开浏览器恒非致命**：`open_in_browser` 失败只记一行，`serve` 照跑——URL 在这之前已经打印。命名不取 `browser`：`crates/browser` 已占住「Agent 驱动真实浏览器」这个概念，一名一义。
- **横幅给人读**：city 目录、WebUI 的完整 URL、客户端完整与否、`Ctrl-C` 停城，四行。bind 是未指定地址（`0.0.0.0`）时 URL 仍给回环形，因为那才是本机打得开的那一个。

**交付形态**：`just package` 产 `sprawling-<version>-<target>.zip`＝二进制＋`start.cmd`／`start.sh`＋`QUICKSTART.md`；裸 exe 不再单独作附件，双击的目标因此永远是启动器。`release.yml` 由 tag 触发，三平台各跑 `just dist`，`xtask budget` 在打包前拦下页壳客户端（`CLIENT_COMPLETE=false` 的二进制），通过后才附件。手工上传的产物来历不明，是本次全部症状的链头，这条把它关掉。

**本章测试**：`default_city` 可写取同级、不可写取 home；`ask` 空行得 `Start`、`q` 得 `Quit`、EOF 得 `Quit`；第一屏文本在返回前已含最终路径（证明「先示后写」）；`local_url` 对未指定地址给回环形。

## 8-9 让二进制成为一个词（P0）

**病灶**：解压之后，那个 exe 不在任何搜索路径上。唯一的入口是找到那个文件夹再双击 `start.cmd`——找一个脚本比敲一条命令难，而桌面快捷方式比两者都难。`sprawling` 今天不是一个可以敲出来的词。

```rust
// bin::install（形状 4 adapter；决定纯，落地薄）
pub(crate) enum PathEdit { AlreadyPresent, Append(String) }
pub(crate) enum PathRemoval { Absent, Rewrite(String) }

pub(crate) fn program_dir(local_app_data: Option<&Path>, home: Option<&Path>) -> Option<PathBuf>;
pub(crate) fn installed_name() -> String;                    // 恒为 sprawling + EXE_SUFFIX
pub(crate) fn plan_append(current: &str, dir: &str) -> PathEdit;
pub(crate) fn plan_remove(current: &str, dir: &str) -> PathRemoval;
pub(crate) fn install(uninstall: bool) -> Result<Report, AxError>;
```

- **一次安装做两件事，撤销就撤销这两件**：把正在运行的这个二进制拷进用户级程序目录，并把该目录写进用户级搜索路径。`--uninstall` 删掉它拷过去的那个文件、删掉它追加过的那一段，别的一概不碰。**恒不要管理员权限**，因为这两件事都在用户自己的 profile 里。
- **装进去的名字是推导的，不是抄来的**：`installed_name()` 恒给 `sprawling` 加平台后缀，不取当前 exe 的文件名。归档里的文件被改过名字，敲出来的那个词也仍然是 `sprawling`——否则「让它成为一个词」这件事取决于谁解压的。
- **搜索路径的判定住 Rust，落地住 PowerShell**：`plan_append`／`plan_remove` 是两个纯函数，输入是那条字符串本身，输出是穷尽枚举。适配器只负责取回原值、写回新值、广播。**幂等因此是一条可单测的判定**，而不是一次要在真注册表上观察的行为。
- **Windows 必须直接改注册表，且必须保住值类型**。`[Environment]::SetEnvironmentVariable(..., 'User')` 是所有教程里的写法，也是错的：它**恒写 REG_SZ**，把本机 `HKCU\Environment\Path` 的 `REG_EXPAND_SZ` 降级，其中的 `%VAR%` 从此不再展开（dotnet/runtime#1442、chocolatey/choco#699）。本机实测该值确为 `ExpandString`，故适配器读原值时用 `DoNotExpandEnvironmentNames`、写回时用读到的那个 `RegistryValueKind`——**读到什么类型就写回什么类型**，键不存在时才取 `ExpandString`（Path 在 Windows 上的默认类型）。
- **值经临时文件进出，不经命令行**：用户名含非 ASCII 字符时，命令行要穿过控制台代码页（本机 936），而 `PATH` 的整条值也可能逼近命令行长度上限。故 Rust 与 PowerShell 之间用一个 UTF-8 临时文件传值，文件路径经环境变量交接，两侧都不需要引号规则。
- **改完必须广播 `WM_SETTINGCHANGE`，否则新窗口也读不到**：Explorer 缓存环境块，从它启动的新控制台继承的是缓存。`#![forbid(unsafe_code)]` 关掉了在 Rust 里调 `SendMessageTimeout` 这条路，故广播由 PowerShell 的 `Add-Type` P/Invoke 完成（`HWND_BROADCAST=0xffff`、`WM_SETTINGCHANGE=0x1A`、`SMTO_ABORTIFHUNG=2`、5 秒上限）。实测一次约 1.1 秒。**广播失败不致命**：路径已经写下了，报一行提示说「注销后生效」，而不是把已经成功的一半说成失败。
- **非 Windows 拷贝照做，改 shell rc 不做**：装进 `~/.local` 下的 `bin`（该目录在现代发行版上默认已在 PATH 上）。**不写 shell rc**，理由记在这里而不是留一个静默的空分支：rc 文件有 bash／zsh／fish 三套语法与 `.profile`／`.bashrc`／`.zshrc` 多个候选，选错就是往人的登录脚本里写一行没有作用却要人自己删的东西；而本机无 Linux/macOS，交叉编译到 Linux 已知走不通（`aws-lc-sys` 需 C 交叉工具链），故这一支只能由 CI 的 ubuntu job 编译与 lint，不能由我运行验收。**没有跑过的写入动作不写**。目录不在 PATH 上时，报告里给出该加的那一行，人自己贴。
- **`Report` 说的是已经发生的事**：拷到哪、搜索路径改没改（`AlreadyPresent` 与 `Append` 是两句不同的话）、广播成不成、以及「PATH 变更不会进已经开着的窗口」。**恒不说「安装成功」四个字**——人要知道的是下一步该开一个新窗口。

**本章测试**：`program_dir` 在两个平台各取本平台约定；`plan_append` 对空串、已含该目录（含大小写不同与带尾分隔符两形）、含其它目录三类输入分别给出正确的穷尽枚举；`plan_remove` 删得干净且保住其余段（含空段）；`plan_append` 之后 `plan_remove` 回到原值——**幂等与可逆是一对性质测试，不是一次手工观察**。

**本章验收（必须真做）**：本机 `install` 之后**开一个新的 PowerShell 窗口**敲 `sprawling`；随后 `--uninstall`，再开新窗口确认 `Get-Command sprawling` 为空。

## 8-10 第二个 wire 客户端（P3）

**为什么存在**：ARCHITECTURE §8 写着「the wire is the whole API；一个第二客户端就照着它写」，而今天只有一个客户端——按仓库自己的判据（§4：一个适配器是假想缝，两个才成立），`channels::wire` 因此是一条假想缝。

```rust
// bin::wire_client（形状 4 adapter）
pub(crate) struct Heard { pub frames: u32, pub refusals: u32 }
pub(crate) fn call(at: &str, frame: &str, token: Option<&str>, quiet: Duration) -> Result<Heard, AxError>;
pub(crate) fn enrol(at: &str, realm: &str, name: &str, value: &str) -> Result<String, AxError>;
pub(crate) fn split_reference(raw: &str) -> Option<(&str, &str)>;   // "realm/name"
```

- **握手在进程内算，不手抄**。`WIRE_V` 与 `schema_hash()` 直接取自 `channels`，故改一条命令名字时本客户端**不可能**落后。本卡因此删掉了那个一次性的 Python 探针——它在工作区外复刻了 `schema_hash()` 与 `IdemKey::derive()`，那本身就是第二个权威。
- **一帧发出，所有帧收回，直到城安静**。“安静”是一段无帧的时长（`--quiet-ms`，默认 2000），而不是帧数：一条 Dispatch 会产生多少事件是城的事，客户端猬不到。
- **输出是 JSONL，一行一帧**。发明一种人看的排版就是为 wire 里的每一个类型再写一遍它长什么样，而那份渲染一定会漂。
- **退出码带信息**：收到过 `Refusal` 退 1，否则退 0。一个驱动它的 agent 不应当为了知道「成不成」去解析 JSON。
- **`enrol` 只从 stdin 读，恒不从 argv 读**。argv 进进程表、进 shell 历史、进父进程的日志；这比浏览器路径更好的地方就在这里，因为页面那条路要先把明文拿进一个标签页的内存。**输出只有引用**，恒不回显值。
- **依赖不新增包**：`tokio-tungstenite` 正是 axum 的 `ws` 特性已经携带的那一份，直接依赖它在 `Cargo.lock` 里**增加零个包**（实测 496 → 496）；换一个别的 WebSocket 库就是把同一个协议的两份实现放进同一个二进制。不开 TLS：控制面走 `ws://`，而一座要经 TLS 到达的城是一座前面站着终结器的城。
- **未做且已知**：`/enroll` 仍在工人取走凭据之前就答 201（详 `channels-SPEC.md` §8）。`enrol` 因此报的是「已受理」而不是「已入库」，这句话写在输出里而不是留给人去撞。

**本章测试**：`split_reference` 对 `realm/name`、缺斜杠、空段、多斜杠四类输入给出正确答案；握手帧的 `wire_v` 与 `schema` 逐字节等于 `channels` 自己的值（这条断言就是「不存在第二份握手权威」的可执行形式）。真城验收：`call` 一条必被拒的命令，收到 `refusal` 且退 1。

## 8-11 控制台：服务中的那个终端不再是死胡同（P1）

**病灶**：`sprawling up` 打四行字然后阻塞到 Ctrl-C。那块屏幕是产品白白扔掉的一个面，也是一台没有浏览器的机器**仅有的**那一个面。

```rust
// bin::console（形状 1 decision；壳在一条线程里，判定全在纯函数）
pub(crate) enum Line {
    Nothing,
    Help,
    OpenWeb,
    Select(Address),
    Quit,
    Frame(Box<channels::ClientFrame>),   // 一个 wire 动词
    Work(String),                        // 普通一行：派给当前选中的 room
    Unknown { verb: String, nearest: Vec<String> },
}
pub(crate) fn parse(line: &str, selected: Option<&Address>) -> Line;
pub(crate) fn verbs() -> Vec<String>;      // 控制动词 ⊕ wire 动词
pub(crate) fn snake(camel: &str) -> String;
```

- **wire 动词表是投影，不是第二份手写清单**。`verbs()` 从 `channels::COMMAND_NAMES` 与 `QUERY_NAMES` 逐个转 snake_case 得来；一份手写清单就是第二套词汇，而它漂开时没有任何东西会发出声音。一条断言钉住这件事：每个 wire 名字都在动词表里。
- **控制动词另成一个穷尽枚举**（`/help`、`/web`、`/at`、`/quit`）。它们是**控制台自己的**动词，不在 wire 上，故不属于那张投影。两张表合并后仍不得重名，一条断言钉住。
- **控制台不做任何判定**。一行变成 `Command` 之后，走的是人在页面上点按钮走的**同一张桌子**（`CommandDesk`）与同一个 `Reply`。拒绝因此自动回到控制台，不需要为它另写一条回程——这正是 P2 那条回信地址的第二个消费者。
- **普通一行就是派活**。要人为一件活敲 `/dispatch {"addr":…}` 是把 JSON 当人机界面；选中一个 room（`/at`）后直接写任务，才是终端本来的手势。未选中任何 room 时拒，并说该敲什么。
- **不是 TTY 就不进控制台**。stdin 读到 EOF（管道、服务、CI）即退出控制台循环而**城照跑**：一座因为没人敲键盘而停止服务的城是一个以交互换服务的回归。
- **拒长表与图**。查询的答案在控制台以 JSONL 逐行输出，与 `sprawling call` 同形；表格与图归浏览器。一个同时伺候两个主人的 CLI 是 CLI 文献里的反面教材。
- **`/web` 携配对令牌**，故没有人需要手拷一串东西。令牌在 `serve` 里只被读一次，控制台拿到的是那一次的副本，不重新读环境。
- **未做且已知**：Ctrl-C 仍是进程终止而非有序收口。`/quit` 是有序的那一条；把信号处理接成有序收口需要一个跨平台的信号依赖，而 `sprawling resume` 已经能收拾一次破死。写成明账。

**本章测试**：每个 `COMMAND_NAMES`／`QUERY_NAMES` 都在 `verbs()` 里（这就是「投影而非第二份清单」的可执行形式）；控制动词与 wire 动词不重名；`snake` 对 `AttachEndpoint`／`RunView` 给出预期串；空行、`/quit`、`/at <addr>`、普通文本（选中与未选中两情形）、未知动词（携最接近的几个）各得正确枚举。

## 8-12 prefix 自己带上它要求模型读的东西（P6.03）

**病灶**（把四个段拼出来才看得见，不是读代码读出来的）：Building 段是 12 字节的地址，Run 段是 71 字节的 `cas:b3-…` 内容哈希。而 City.md 要求模型「read `BUILDING.md`」「`FULL READ:` 给出你的 `JOB.md` 的路径」——**两句话指的东西一个都不在 prompt 里，而城里八个工具没有一个解析 `cas:`**。第三处：City.md 无条件说「你的第一条消息正好有三行」，这对没有 `JOB.md` 的主 Agent 是假的。

```rust
// bin::assembly（形状 4 适配器；四个段的填充点）
fn building_segment(city_root: &Path, addr: &Address, building: &Address) -> Vec<u8>;
fn run_segment(city_root: &Path, building: &Address, brief: &city::RunBrief) -> Vec<u8>;
```

| 槽 | 装什么 | 稳定性依据 |
|---|---|---|
| City | 城里那份 `City.md` | 整座城不变 |
| Building | 地址 ＋ `.sprawling/BUILDING.md` 全文 | 人写、任何写域够不到、整个 Run 不变 |
| Resident | `URBANITE.md`（或无身份那 106 字节）＋ catalog | 同一个 Resident 每次 Run 同样的字节 |
| Run | `Handoff.md`（写过的话）＋ 本次 brief | 每次 Run 一份 |

- **注入的判据是稳定性，不是重要性**。`Roadmap.md` 与 `Memo.md` **故意不注入**：Run 自己会改它们（`plan` 工具持有 roadmap 全文），冻进 prefix 就是第二个权威——模型会读到自己刚改过的旧副本。它们的正路是工具，不是 prefix。
- **Run 段的次序是「上一场留下的」在前、「这一次要做的」在后**：最后读到的东西是被执行的东西。
- **空白表单不占 prefix 字节**：`city::handoff` 认出还是模板原样的 `Handoff.md` 并答 `None`。判据是模板自己的括号提示行——写过的会话会把它们换掉。
- **内容哈希整个退出 prompt**。它在 Ledger 里记了两遍（`run.rs:126` 的 pin 与 `:141` 的 started），溯源不依赖模型看见它；`FULL READ:` 那一行随之消失。
- **CAS 的 pin 改钉 brief 的正文**，两条臂都钉：一次没人派任务的会话，pin 里是「说明没有人派」的那几句，于是 Ledger 的 `job` locator 恒解析得到 Run 段真正携带过的字节，而不是一个从未被写出的文件。

**本章测试**：一次真派活后，provider 收到的请求里含楼规原文（`confidential: false`）、含上一场的 Handoff 正文、含本次 Goal，且**不含** `FULL READ` 与 `cas:b3-`；一次无 Goal 的派活不写 `JOB.md`，请求里说出「working with the person directly」且不把人那句话包成 `Task:` 表单。

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

P0（`bin::install`）引入四处，全部是外部世界的事实而非我们的选择，故各自注明出处：`%LOCALAPPDATA%\Programs\<app>` 是 Windows 用户级程序目录的约定；`~/.local` 下的 `bin` 是 XDG 用户级可执行目录的约定；`HKCU\Environment` 是用户级环境变量在注册表里的位置；`WM_SETTINGCHANGE=0x1A`／`HWND_BROADCAST=0xffff`／`SMTO_ABORTIFHUNG=2` 是 Win32 的常量值。这四处一旦被平台改掉，改点各只有一个。

## 15 影响面

justfile／CI 无涉；S4 前端框架结论书将改写 build.rs 拷贝源与 `just build-web`。

## 16 测试与约束

单测：嵌入字节非空且含 `sprawling` 标记。约束：workspace lints 全量适用（含 build.rs）。

## 17 模型体验

零字节：bin 不产生任何入窗内容。

## 18 文档同步

子命令每扩一个：本 SPEC 增章、ARCHITECTURE.md §12 状态翻转、CLI 三栏表核对。

P7 起交付形态入册：`just package` 的产物名、`QUICKSTART.md`、README 与 `docs/getting-started.md` 的首次运行段、`release.yml` 的附件清单，五处同改。
