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

**已有工作区的人（P0.01）**：`form_city(root, Adopt)` 取代 `init_city` 成为唯一的成城路径（后者是 `Adopt::Nothing` 的别名）。`Adopt` 是穷尽枚举而不是布尔：在既有工作旁边形成一座城、与把那些工作放到规则之下，是两件事，一个布尔会把它们说成一件事的一个设置。采纳走的是 `sprawling adopt` 的同一道门，于是创世时收进来的文件夹与一个月后收进来的受同一套规则治理。首屏因此长出第三个答案 `FirstScreen::Use(path)`——回车之外、`q` 之外的任何输入都是一个路径（去掉文件管理器加的引号）；**路径存不存在由调用方查并报**，屏幕自己去猜要么把真文件夹当错字拒了，要么在没人看过的位置造一座城。

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
- **Ctrl-C 已是有序收口（P3.05）**：`serve` 在 `channels::serve` 与 `tokio::signal::ctrl_c` 之间 `select!`。收到信号后先停止接受连接，再 `CommandDesk::close()` 告诉 worker，worker **在读队列的同一处**读到它，于是正在跑的那条命令先跑完，`handoff_written` 是最后一行而不是某一行的中间。主线程 join worker 线程再返回——先返回的 main 会在那一行写出来之前结束进程。
  - **`DeskWait::Close` 与 `Gone` 不是一回事**：前者是人选择停城，值一份 Handoff；后者是桌子自己坏了，那座城已经写不出 Handoff 了。
  - **收口不是一条 Command**：能被拼出来的线上帧就是陌生人停掉别人城市的一条路。`closing` 是台子上的一个 `AtomicBool`，只有起城的那个进程按得动。
  - **Windows 交两个信号，本城两个都收**：控制台会发 Ctrl-C 与 Ctrl-Break。一座在其中一个上有序收口、在另一个上暴死的城，等于同一个手势有两种行为，而决定用哪一种的是人碰巧按了哪个键。其他平台只有一个。
  - **裁定与代价**：根 `Cargo.toml` 给 tokio 开 `signal` feature。TODO 当时写的「不新增包」是错的——unix 上它引入 `signal-hook-registry`（Apache-2.0/MIT，deny 表内），依赖数 496 → 497。这一条如实记在这里。

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

## 8-13 一封信与一次敲门（P3.07）

**病灶**（一次真机会话拿出来的，不是读代码读出来的）：两位居民在同一栋楼里谈价，发信的那一跑连续五次 `signal pull` 等一封**在它自己那一跑里物理上不可能到达**的回信，最后以 `limit` 冻结；而收信人根本没有在跑。证据：同一条链上 `signal_enqueued` 落在 `run_frozen` 之后两行。

```rust
// bin::assembly
struct Knock { addr: Address, from: String, mode: runtime::Mode, budget: kernel::BudgetCap }
impl RunWorker {
    fn knock(&mut self, signal: &Signal, speaker: &Address, mode, budget) -> Result<(), AxError>;
    fn answer_knocks(&mut self);   // 成波排干，循环而非递归
}
```

**两种送达，分法是收信人在不在**：

| 收信人的状态 | 机制 | 落点 |
|---|---|---|
| 正在跑 | 信从门缝塑进去——steer 型 Signal，`SignalDesk::take_steer` 在安全点取走 | 追在下一次工具结果末尾，前缀 `@发件人地址` |
| 没在跑 | 敲门——投递后入 `knocks`，本轮派活结束后 `answer_knocks` 为他开一跑 | 新 Run 的 brief，同样写明 `@发件人地址` |

- **人压过居民**：中断源先问人的命令队列（Cancel 再 Steer），空手才问本屋信箱。
- **属名不是装饰，是回信地址**：另一个 agent 的话氒不得以人的身份进窗口。类型已经把它变成判定（只有 `Steer::from_person` 写得出 `user`）；本卡把同一条规则延到敲门路上——被叫醒的一跑，其 brief 第一句就是「@X signalled you. This run exists because that signal arrived: nobody else asked for it.」。一份读起来像人写的 brief 会让每一封回信寄错地方。
- **敲门敲的是 Resident，不是一段已封存的对话**：冻结的 Run 是历史，历史只读而不叫醒；被开出来的是那个地址上住户的**一跑新的 Run**，它靠 `Handoff.md` 接住上一场——那正是为穿过一次冻结而造的那件东西。没有 `URBANITE.md` 的地址因此不敲：它是一间房而不是一个人，信就在那儿等到人派个住户过去。
- **不设叫醒预算（人的定谳）**：什么时候该停下来是对话里那几位居民的事，城市的活是把话送到。人要让某个居民不再被打扰，用的是已有的 Halt，`dispatch_in` 当场拒一个被 halt 的 scope。
- **一次对话只有一道底，而它数的不是钱**：每一跑受 `DISPATCH_TURN_BUDGET`（24 回合）约束。`kernel::gate` 的 spend 门至今零调用方——**这座城没有金额上限**，那是定谳而不是遗漏：什么时候停下来归对话里的居民，花了多少事后从 Ledger 报出来。
- **一个敲不成不连坐发件人**：叫不醒的人进诊断日志，不把发件那一跑的 dispatch 弄成失败。

**本章测试**：一位居民向另一位发信，无人再派活而收信人自己跑了一跑，且其 brief 里带着发件人的地址；向一个无 `URBANITE.md` 的房间发信不开任何 Run，信仍在队里。

## 8-14 幂等键里的那个时钟（P3.08）

**病灶**（真机会话抓出来的）：同一跑里两次 `read` 被拒为 `this call was already made`，下一回合同一路径又读得干净。原因在一行里：`IdemKey::derive(&run_id, Seq::new(t.value()), call.name.as_bytes())`——

一、**它取了一个时钟**（回合的毫秒戳），而确定性第七条写着「IdemKey 恒不得源于时钟或随机数」；二、**它不含参数**，于是同一回合内对同一件工具的任两次调用归为一键——两次 `edit` 也会，而那是丢写。

现形：`(run_id, 本跑内的调用序号, 工具名＋参数 JSON)`。序号由闭包自己的计数器给，重放同一段历史得同一串键。两次不同的调用是两个键，都跑；同一个位置被重放是同一个键，去重正是为此而存在。

## 8-15 装配层长出一扇门（整修卡 R2.01）

```rust
// crates/sprawling/src/lib.rs —— 索引文件，只准声明（modmap 已看守）
pub mod assembly;
pub mod console;
pub mod firstrun;
mod mcp_http;      // 只经 assembly 到达
mod mcp_stdio;     // 同上

// assembly：跨出 crate 的项，逐个放行
pub struct InitReport { pub ledger_dir, pub genesis, pub standing, pub adopted }
pub enum Adopt { Nothing, EveryFolder }
pub fn has_history(&Path) -> bool;
pub fn init_city(&Path) -> Result<InitReport, AxError>;
pub fn form_city(&Path, Adopt) -> Result<InitReport, AxError>;
pub fn open_vault() -> (gateway::Custodian, Option<Payload>);
pub struct Serving { /* 八个字段全 pub：调用方构造它 */ }
pub async fn serve(Serving) -> Result<(), AxError>;
pub struct ScanReport { pub waiting_approvals: usize /* lines、closed_calls 不跨出 */ }
impl ScanReport { pub fn summary(&self) -> String; }
pub struct RunWorker;
impl RunWorker {
    pub fn new(&Path, gateway::Custodian, Diagnostics) -> Result<Self, AxError>;
    pub fn handle(&mut self, channels::Command) -> Result<(), AxError>;
    pub fn startup_scan(&mut self) -> Result<ScanReport, AxError>;
    pub fn fork(&mut self, RunId, Seq, Option<Address>) -> Result<RunId, AxError>;
    pub fn adopt_building(&mut self, Address) -> Result<(), AxError>;
}

// console
pub struct Terminal { pub url: String, pub token: Option<String> }

// firstrun
pub enum FirstScreen { Start(PathBuf), Use(PathBuf), Quit }
pub fn ask<R: BufRead, W: Write>(&Path, &mut R, &mut W) -> std::io::Result<FirstScreen>;
pub fn default_city(&Path, Option<&Path>, bool) -> PathBuf;
pub fn is_writable(&Path) -> bool;
pub fn local_url(SocketAddr) -> String;
pub fn open_when_ready(SocketAddr, String);
```

- **这张卡为什么存在**：`crates/sprawling` 至今只有 `src/main.rs`，`mod assembly` 是私有模块，于是工作区里**没有任何东西能依赖它**——4377 行生产代码（含 1058 行的 `dispatch_in`）只由同文件内的 66 个测试看守，citysim 与任何 `tests/` 都够不到。同一个事实还有第二个后果：它是唯一带 SPEC 却逃过 `apisync` 的 crate，因为 `spec_crates` 以 `src/lib.rs` 是否存在为判据。加一个 lib target 一并了结两件。
- **`pub mod` 而非扁平 facade**：§12 模块表以 `bin::assembly`／`bin::console`／`bin::firstrun` 命名模块，模块名本身是已记录的架构事实；折成 `sprawling::init_city` 会抹掉这层限定，而本 crate `publish = false`，C-REEXPORT 要替第三方省的那段路径没有受益人。**取窄的地方在项，不在模块**：只有跨出 crate 的项改 `pub`，其余留 `pub(crate)`——公开面因此是逐项决定的，不是逐模块授予的。
- **`mcp_http` 与 `mcp_stdio` 保持私有**：只经 `assembly` 到达（`assembly.rs` 的 `McpLink`），没有第二个调用方。
- **`install` 与 `wire_client` 留在 bin**：前者把二进制放上 PATH，后者从终端连一座已服务的城并从 stdin 读 enrolment——两者都是关于命令行的，不是关于城的，且除 `main` 外零引用。留在 bin 让公开面少六项。
- **`handle` 进公开面不是为测试拓宽**：AGENTS.md 写着「Tests use the same doors as production code」。`handle` 正是服务中的 worker 循环走的那扇门，把它命名出来是承认已有的门。反过来，那 66 个内部测试**不搬去 `tests/`**：它们触及 `rebuild_views`／`read_building`／`run_id_for` 这类内部项，搬迁会为测试拓宽公开面，正是同一条规矩禁止的事。本 crate 的文件长度因此在本卡内不变——它变短要等拆 `dispatch_in` 那张卡把生产代码连同其测试一起搬走。
- **`ScanReport` 只放行一个字段**：`main` 读 `waiting_approvals` 决定是否多印一行，`lines` 与 `closed_calls` 只进 `summary()`。按需放行而非按结构对齐——`InitReport` 四个字段全跨出，是因为 `report_standing` 四个全读。
- **零行为变更**：`main.rs` 只改开头的声明块（七行 `mod` → 两行 `mod` ＋ 一行 `use sprawling::{assembly, console, firstrun}`），其余调用点逐字节不变。`Cargo.toml` 不改：Cargo 对同一 package 自动发现 `src/lib.rs` 与 `src/main.rs` 两个 target，OUT_DIR 对两者相同，`include!(client_embed.rs)` 与 `DEPENDENCIES` 因此留在 `main.rs` 原地。
- **红**：`crates/sprawling/tests/assembly_door.rs` 走 `init_city → RunWorker::new → handle(Command::CreateBuilding) → 读 InitReport.ledger_dir 下的账本`，断言 `building_created` 落账。本卡之前它连编译都过不去（`sprawling` 这个 crate 名不存在），这就是「这条测试咬得动」的证据。
- **门禁连带**：`apisync` 自本卡起把 `sprawling` 纳入契约，`xtask/api-baselines/sprawling.txt` 随本卡生成（`guard` 的 `PRODUCED_PREFIXES` 已豁免该目录，不需 `Verdict:`）；`header` 要求 `lib.rs` 与新测试文件各带三行 MPL 通告；`modmap` 对 `*/lib.rs` 自动按索引文件判定，只准 `mod`／`use`／`pub use`／注释／属性——facade 因此只能是声明，正是要的形状。
- **一处文档更正**：ARCHITECTURE.md §3 写着「citysim is a second assembly layer: the same code with simulated adapters」。此句与现实不符——`citysim/Cargo.toml` 依赖 kernel／memory／runtime／gateway／eval，其中没有 sprawling；`run_scenario` 手工构造 `RunPlan`，够到的最高层是 `runtime::run::drive`。本卡使 assembly **可被依赖**，但没有让 citysim 依赖它：模型适配器仍由 `adapter_for` 从 `EndpointBook` 内部构造，那条缝要不要倒置是另一个决定。按 AGENTS.md「reality wins and the document is corrected first, with its reason」，本卡先把这句改成现实。

## 8-16 读不了的计划不再被报成被人改过的计划（整修卡 R2.06）

**病灶**：`dispatch_in` 里三处把失败抹平成默认值。

```rust
let plan_text = std::fs::read_to_string(&plan_path).unwrap_or_default();          // 驱动前：喂给 ClaimDesk
let shelf = city::archive_index(&self.city_root, building.addr()).unwrap_or_default();  // 驱动前：喂给 ArchiveDesk
let on_disk = std::fs::read_to_string(&plan_path).unwrap_or_default();           // 驱动后：落盘前的 compare-and-swap
```

第三处最重。那一段的注释自述它存在的理由——「each effect is checked against the file **as it stands now** … the losing claim is dropped with a diagnostic instead of overwriting somebody's row」。但读失败使 `on_disk` 成为空串，`still_true` 对空文档恒为 `false`，于是每一条 claim 都落入 stale 分支，人收到的诊断是「row … moved before this run's claim landed」——**一个从未发生的并发冲突**。他们会去查另一个居民，而真正要修的是一个读不开的文件。

（我先假设的是更重的后果——读失败→`stale` 为空→`write_plan` 覆盖真实计划。核实 `still_true` 后否定了它：空文档下 `check_roadmap_shape` 不产 `WellFormed`，因此恒返 `false`。不存在数据丢失，只存在误报。）

第二处：`city::archive::index` 自己已经实现了正确契约（目录不在 → `Ok(空)`，真失败 → `Err`），所以 `.unwrap_or_default()` 恰好只扯掉真失败；换成 `?` 即可，不需新机制。

**现形**：

- `assembly::plan_path` 删除。它在 `city` 之外拼了一遍 `city_root/<addr>/Roadmap.md`，而 `ROADMAP_FILE` 住在 `city::spine_files`——两份「计划在哪里」的权威。改走新增的 `city::roadmap_path`。
- 两处读全走 `city::roadmap`：仅 `NotFound` 答空串，其余以 `E_STORAGE_FATAL` 上报并带路径。一栋还没铺计划的楼确实没有计划，那不是失败；其余一切都是。
- `archive_index(…).unwrap_or_default()` → `?`。

**拒而不是降级**：计划是共享地面。读不到它就开跑，会花掉一次模型调用去产生一批注定被丢弃的 claim。在派活口上拒，人拿到的是路径和修法。

**红**：向一栋 `Roadmap.md` 是**目录**的楼派活（`read_to_string` 因此以非 `NotFound` 失败，无需权限把戏）。本卡之前：派活成功，诊断行说「row moved」。本卡之后：派活被拒，错误点名那个文件。

**影响面**：`city` 公开面增两项（基线同提交更新）。正常楼不受影响——`spine_files::lay_out` 给每栋新楼都铺了 `Roadmap.md`，而未铺的情形仍走 `NotFound` 答空串这条。`assembly.rs:219`（楼页读 Roadmap）同属一族但爆炸半径不同——那里读不到只是页上少一块，不会变成误报——本卡不动。

## 8-17 一次验证遍历，三个折叠（整修卡 R2.02）

```rust
pub(crate) struct Standing { pub(crate) book: gateway::EndpointBook, governance: Governance, collaboration: Collaboration }
impl Standing { pub(crate) fn fold(ledger_dir: &Path) -> Result<Standing, AxError>; }

impl Governance { fn empty() -> Governance; fn absorb(&mut self, record: &EventRecord); }
struct CollaborationFold { … }   // 暂存 enqueued／consumed，`settle` 产 Collaboration
```

`rebuild_book`／`rebuild_governance`／`rebuild_collaboration` 三个函数删除。

- **这张卡不是缺陷修复，我测过了**。我原本怀疑三处实现会漂移（`rebuild_governance` 管 `granted` 与 `CityHalted`，`govern` 不管，`answer_approval`／`set_admission` 各自直改字段）。新测试 `what_a_worker_holds_is_what_a_restart_rebuilds` 实验否定了它：派一次活、发一条信号之后，活 worker 与重建结果逐项相等。那条测试因此不是本卡的红，而是让合并安全的护栏；它同时把一条四处代码都依赖、却从未被断言过的形状-7 性质变成了可红的。
- **本卡以测量收口而非以红转绿收口**，理由写在上一条：没有可咬的红，因为没有缺陷。本机实测（windows-x86_64, 16 core，release，外部探针经 `sprawling` 的 lib 门驱动 `RunWorker::new`）：

  | 记录数 | 改前 | 改后 |
  |---:|---:|---:|
  | 5,000 | 136.6 ms | 44.6 ms |
  | 20,000 | 538.7 ms | 175.6 ms |
  | 50,000 | 1856.1 ms | 436.2 ms |

  这是每一次 `serve`、`resume`、`fork`、`adopt` 都要付的钱。
- **为何不是 4 → 1 而是 4 → 2**：`RunWorker::new` 自己还要 `JsonlLedger::open`（尾部恢复）读一遍，而 `serve` 另走 `rebuild_views` 一遍。把 `Views` 也并进来要改 `serve` 的所有权形状（它住在 `Arc<Mutex<_>>` 里与查询侧共享，而 worker 在自己线程上）——那属于 `dispatch_in` 拆分那张卡，不在本卡内顺手做。
- **暂存只给真需要的一项**：`CollaborationFold` 只暂存 signals，因为队列是 `enqueued` 减 `consumed` 而两者到达顺序任意；book、governance、goals、requests 都是逐条即结的，所以不暂存。
- **验证不动位**：链验仍在折叠之前。一部不能自证的历史，不是这三个视图中任何一个可以建在上面的历史。

## 8-18 审查中的 run 不再把自己的决策直接放上楼的书架（整修卡 R2.03）

**病灶**：`dispatch_in` 的档案回收段写的是 `city::file_archive(&self.city_root, …)`，而不是 `&write_root`。ARCHITECTURE.md §5 把输线设计写在明处——「A building under review gives every run its own tree … Nothing it writes is visible until somebody else checks it — the losing line of the design made physical rather than promised」。档案不在那棵树里。

具体危害：书架回头喂给 `ArchiveDesk`，成为模型看到的「这栋楼已经知道什么」。一个**被驳回**的 PR 里的决策因此会留在架上，变成后续 run 的前提。

**现形两步，缺一不可**：

1. `city::file_archive(&write_root, …)`——落进围栏。
2. 持有租约时，`fence_scope` 从**房间**改为**楼**。否则第一步把泄漏换成了静默丢失：`wave_pre` 只暂存 `<scope>/*`，而档案在 `<building>/Archive/…`，在房间作用域之外，不会进提交，租约一释放就没了。在**自己独占的** worktree 里暂存整栋楼是安全的：那棵树里变化过的东西全是这个 run 的。无租约时围栏仍在房间——那才是一个 run 在城里唯一可写的地方。

**与计划的不对称是故意的**：`Roadmap.md` 恒写回城里，因为它是共享地面且带着对当前文件的 compare-and-swap（那段注释自述了理由）。档案没有这样的声明，也没有守卫，所以它是漏而不是决定。

**红**：在 `review: true` 的楼里派一次带 `archive` 工具调用的活，断言书架仍空。本卡之前它拿到 `[Entry { kind: Decision, … lab\Archive\decision\… }]`。同一条测试接着让第二位居民检查并合入，断言书架变为 1 条——**两半同一条测试**，因为只测前半的修法可以是「干脆不写」。

## 8-19 沙箱接上（整修卡 R2.04）

**病灶**：`dispatch_in` 把 `Box::new(runtime::AbsentSandbox)` 写成字面量，而 `crates/sprawling/Cargo.toml` 没有任何 feature 到达 `runtime/wasm`。于是 `runtime::WasmtimeSandbox` 在 `runtime/tests/sandbox_a10.rs` 之外**没有调用方**，任何 sprawling 构建都到不了执行引擎。

`AbsentSandbox` 的三段式拒绝里写着 recovery：「use the program arm, or install a build with the `wasm` feature」。**那样的构建不存在。** 这正是 P4.03 立下的判据——只写在文里、无人执行的 recovery 等于没有 recovery——这次是只写在错误消息里、无人可安装的构建。

ARCHITECTURE.md §2 把 wasmtime 列进技术栈并声明了代价（「Cost: an optional feature; a build without it refuses tool execution in three parts rather than pretending」），措辞预设了存在带该 feature 的构建。

**现形**：
- `crates/sprawling/Cargo.toml` 增 `[features] sandbox = ["runtime/wasm"]`。
- 引擎的选择收进一个函数 `execution_engine()`，两条 `#[cfg]` 臂各一个实现，`dispatch_in` 的构造点因此不随 feature 改变形状。
- **带引擎的构建起不来引擎就拒派活，不回落**。回落是「人以为跑在沙箱里、其实没有」的由来。

**默认仍为关**：这是 ARCHITECTURE.md 记录过的取舍（wasmtime 是一大块二进制），本卡不改默认，只让开关存在。`just check` 走 `--all-features`，所以带 feature 的那条臂进门禁；`just dist` 不带，所以交付形态与体积预算不变。

**红是编译红而非行为红，这里说明白**：改动前 `execution_engine` 不存在，测试连编译都过不去。行为面的红需要一个真的 `python.wasm` 与 `SPRAWLING_PYTHON_WASM`，那是交付形态的事，不在本卡内。新测试只在 `cfg(feature = "sandbox")` 下存在，断言引擎给出的不是「this build carries no execution engine」那句话。

## 8-20 交接件读不了不再等于没有交接件（整修卡 R2.05）

`city::handoff` 的 `.ok()?` 与 R2.06 修掉的那三处同族，且它喂的是 **prefix 的 run 段**——下一次会话读到的第一样东西。三件事（文件不在／读不了／仍是空白表单）原先并为一个 `None`。

现形与 `roadmap` 同：`Result<Option<String>, AxError>`，`None` 只说「没有值得带走的东西」，`NotFound` 归入其中，其余上报并带路径。同时补 `handoff_path` 与把 `HANDOFF_FILE` 转 `pub`——红测要点名那个文件，而在别处拼一遍文件名就是第二份权威。

`run_segment` 因此转为 `Result<Vec<u8>, AxError>`；它只有一个调用方（prefix 的四段装配），所以波及面就是那一处 `?`。

**红**：向一栋 `Handoff.md` 是目录的楼派活，`expect_err` 撞上 `Ok(())`。`city` 侧另加一条单测，把三件事排成三行断言。

## 8-21 控制台读得到它身处的那座城（整修卡 R2.07）

**病灶（两处，同一个不对称）**：控制台与 socket 拿的是同一张桌子（`CommandDesk`）与同一条事件流，唯独**读**这一路没接上。

1. `console::post` 对 `ClientFrame::Query` 只印一句「a question is answered over the wire: `sprawling call '…'`」——它请人开第二个终端，去问一座人已经身处其中的城。而 `assembly::serve` 早已构造出 `queries: Arc<dyn Fn(Query) -> Result<Answer, AxError>>` 并只交给 socket。§8-11 自述「查询的答案在控制台以 JSONL 逐行输出，与 `sprawling call` 同形」——**这句话今天是假的**，本卡使它为真。
2. `serve_city` 起城时印的四行（city／WebUI／client）随事件流滚走。一个远程盯着城的人于是再也看不到自己开在哪个端口、有几条 run 在跑。

```rust
// bin::console（形状仍为 1 decision；渲染是纯函数，I/O 仍在壳里）
pub struct Terminal {
    pub url: String,
    pub token: Option<String>,
    pub city: String,     // 新增：城在磁盘上的位置
    pub client: String,   // 新增：客户端从哪来（嵌入／目录）
    pub bind: SocketAddr, // 新增：真正绑住的那个地址
}

/// socket 用的那一个答询函数，控制台拿到的是它的副本。
pub(crate) type Answering =
    Arc<dyn Fn(channels::Query) -> Result<channels::Answer, AxError> + Send + Sync>;

pub(crate) enum Line { …, Serving }   // 控制动词从四个变五个

/// 进程自己知道的事实 ⊕ 一次 Metrics 的答案 → 一屏。纯。
pub(crate) fn serving(terminal: &Terminal, vitals: &channels::MetricsAnswer, pid: u32) -> String;
```

- **答询走同一个函数，不是第二个权威**。`post` 的 Query 臂改调 `Answering`，与 `channels::server` 的 `SessionStep::Answer` 是同一个 `Arc`；控制台答出来的数字与浏览器看到的数字不可能不同，因为它们是同一次调用。
- **`/serving` 是渲染，不是来源**。城侧那几个数（几条 run 在跑、几件事等人、几栋楼）全部来自一次 `Query::Metrics`；`/metrics` 仍印它的 JSONL 原样，与 `sprawling call` 同形。两个动词，两个问题，无重叠：`/serving` 答「这个进程开在哪、门朝谁开」，`/metrics` 答「这座城里有多少什么」。
- **动词名不与既有概念撞车**。`status` 在 `docs/glossary.md` 里已经是**工具**的名字（「答一次 run 自己的处境」），一名一义是门禁事项，故控制台这个动词叫 `serving`——它印的正是 `assembly::Serving` 持有的那几样东西，沿用已在库内的词。
- **常驻内存不进这一屏，理由记在这里**。「resident 在本平台叫什么」的唯一权威是 `xtask::mem`（Linux `smaps_rollup` Pss／macOS `ps rss`／Windows `WorkingSet64`），而 `xtask` 只依赖 `kernel`——让它依赖产品会使每次门禁编译整个 workspace。在 bin 里再抄一张三平台表，正是那个模块自己的 doc comment 警告的「三份权威」。`/serving` 因此印出本进程 **pid**，`cargo xtask mem <pid>` 只差一次粘贴。**翻案条件**：人裁定新增第十三个 unit 承载这一个计数器（ARCHITECTURE.md §3 的拓扑是 add-only 且需裁决），届时两个调用方共用一份定义。

**红**：一条测试把 `Line::Serving` 之外的路径全部钉住不动，另一条驱动 `drive` 读入 `/metrics`，断言输出里有 `MetricsAnswer` 的 JSON 而**不含** `sprawling call`——本卡之前它撞上那句转介。第三条断言 `serving()` 的那一屏同时含端口、`runs`、与 pid。

## 8-22 面向网络的那扇门自己铸钥匙，页面把钥匙递上去（整修卡 R2.08）

**病灶（一条端到端全断的链，四段里断三段）**：把 WebUI 暴露到回环之外这件事，今天**做不到**。

| 段 | 今天 | 判据 |
|---|---|---|
| 铸 | `PairingToken::mint` **在产品里没有调用方**，只有测试用它 | `grep mint(` 只命中 `channels/tests` |
| 拒 | `decide_bind` 在没有 `SPRAWLING_PAIRING_TOKEN` 时拒绝任何非回环地址 | `server.rs:63` |
| 携 | `console::web_url` 把 `?token=…` 挂到 URL 上 | `console.rs` |
| 递 | `web::app` 写死 `Link::new(None)`，且 `socket_url()` 只取 `host`，**查询串整段丢掉** | `app.rs:1679`、`socket.rs:311` |

于是：不配置令牌起不来；配置了令牌，页面握手时不出示任何东西，`decide_handshake` 照 `server.rs:306` 拒之。**一座暴露出去的城连自己的 WebUI 都进不来。** 这不是两个缺陷，是一条链，所以一张卡修完，不留「钥匙铸出来了但没人能用」的中间态。

**对用户提案的更正，记为落选方案**：「BLAKE3 随机抽一个文件的 hex 值当口令」的熵是 `log2(候选文件数)`，不是摘要宽度——十万个文件约 17 bit，可当场穷举；且被抽中的文件内容常常是公开的（仓库里的源文件、依赖的许可证）。攻击者只要知道文件集合就把 256 bit 的外观还原成一次目录枚举。`PairingToken::mint` 收 32 字节 OS 熵、经 29 符号字母表给出四组五位（约 97 bit），**且它的 doc 明写就是为「显示一次」而设**。本卡用它，不另造。

```rust
// bin::keying（形状 1 decision；纯，穷尽，无 I/O、无熵）
pub(crate) enum Keying {
    /// 回环：这台机器自己，什么都不用出示。
    NothingToPresent,
    /// 人配置过的：我们没见过它被铸出来，故不显示。
    Adopt,
    /// 这次服务当场铸一把，显示一次，不落任何地方。
    Mint,
}
impl Keying { pub(crate) fn decide(bind: SocketAddr, configured: bool) -> Self; }

// bin::assembly（形状 4 适配器；熵在这里取，与 `random_token` 同一处出身）
pub enum Keyed { NothingToPresent, Adopted(String), Minted(String) }
pub fn key_for(bind: SocketAddr, configured: Option<String>) -> Result<Keyed, AxError>;
```

- **`decide_bind` 不动**。它仍是那条守卫；assembly 只是**在 socket 存在之前先把它满足了**，所以它 doc 里那句「there is no window in which the port is open and unauthenticated」原样成立。拒绝臂在 `channels` 自己的测试与任何第三方 embedder 处仍可达。
- **人配置过的优先**。`SPRAWLING_PAIRING_TOKEN` 在场就 `from_configured` 采纳，不覆盖、不显示——我们没见过它被铸出来，印它就是把一个长期口令又抄进一处日志。
- **铸出来的 code 不落盘、不进 Ledger、不进 diagnostics**。进程结束即失效，这就是「一次性」。它只经两处：显示一次的那一行，与 `Terminal.token`（`/web` 据此拼出带钥匙的 URL）——后者今天已经持有明文，本卡不扩大它的存放面。
- **页面这一段是纯函数加一次读**（Humble Object，ARCHITECTURE.md §9）：`web::socket::token_in(search) -> Option<String>` 对查询串取值，可在非 wasm 目标上测；`pairing_token()` 只在 wasm 下多一次 `location.search()`，不含判定。`app.rs` 的 `Link::new(None)` 改为 `Link::new(crate::socket::pairing_token())`。
- **令牌留在查询串里是既有裁决的延续**，不是本卡新开的：`console::web_url` 的 doc 已写明这一取舍（「A token in a query string is a token in the browser's history, and that is the trade this makes deliberately」），替代方案是人在两个窗口之间手抄一个秘密，然后抄错并粘到更糟的地方。改存 `sessionStorage` 会把钥匙放进同源 JS 读得到的地方——对一座**公网暴露**的城，那比浏览器历史更坏，故不改。

**红（三条，每条咬住一段）**：`Keying::decide` 对四格（回环／暴露 × 配置过／没有）给出的枚举——本卡之前 `keying` 不存在，是编译红；`token_in` 对 `?token=abc`、`?a=1&token=abc`、`?token=`、空串的四个答案；以及 `web::socket` 那条握手测试，断言 `Link::new(token_in(...))` 发出的 `Hello.token` 非空——本卡之前 `Link::new(None)` 使它恒 `None`。端到端那一段（真浏览器对真暴露端口）落在 V9，是人跑的命令而非门禁，如 ARCHITECTURE.md §11 所记。

## 8-23 委派下去的活带着派它的那份预算（整修卡 R2.09）

**病灶**：`knock` 携父 run 的 `budget`，其注释明写「Carried rather than defaulted: an answer belongs to the same piece of work as the question, and a run with no ceiling is the one failure with no floor under it」；而同一个文件里 `dispatch_in` 的**委派**分支写 `kernel::BudgetCap::default()`。委派比敲门更是同一件活——一个 delegate 就是替父 run 做事的——却是唯一被清零的那条路。

**它今天可达，不是潜在的**。`channels::WireCommand::Dispatch` 带 `budget` 字段，`sprawling call` 与 `protocol::acp` 都能填。人在页面上填不了（ARCHITECTURE.md §5 步 1：「The frame carries no budget」），但**页面不是唯一的客户端**，而 wire 就是全部 API。

**危害的形状是「对模型说假话」，不是超支**。`BudgetCap` 今天没有执行者：`kernel::budget` 的 ladder 与 `SpendVerdict` 只有 `kernel::gate` 自己的测试在走，它在本 crate 里唯一的消费者是 `StatusTool`。所以一个 delegate 向模型报告自己预算为零，而它的父 run 报告的是真数。这与 §8-12 记下的那一类同族：「City.md 让模型调 `status` 问这些，而一个照做的模型拿到一排零，于是学会不再问」。修它不是为了今天省钱，是为了那一行不再是假的。

**修**：`dispatch_in` 的委派把 `budget` 传下去，与 `knock` 同形。`BudgetCap` 是 `Copy`，所以是一个词。

**红**：以 `BudgetCap { usd: 250_000, tokens: 4_000 }` 派一次会委派的活，父子两个 run 各调一次 `status`，按**状态块里的 `addr:`** 分辨谁读的哪一行。父为对照组（两边都绿），子为受试组（本卡之前是 `0 usd_micros, 0 tokens`）。

——**数个请求体里包含那串字」不能作判据**：对话携带自己的历史，同一个 run 的 status 答案会出现在它之后每一次请求里，数体等于把父数了两遍。我的首版测试就是这么写的，**未改代码即绿**，记在这里以免重踩。

**本卡另查出一处更深的，归入待办**：`fn dispatch(addr, task, goal)`（审批应答后续活的那条路）同样写死 `BudgetCap::default()`，而它**无法只靠改一个词修好**：`BlockedJob { addr, task, goal }` 没有装天花板的字段，而 `blocked_job` 是从 `run_started` 的账本记录重建它的——那条记录里没有预算。于是一个带天花板派出、因委派而停下来等人批的 run，被批准后续上的那一跑天花板归零。

**它与交接件上的第 4 项（`blocked_job` 不再扫全史）是同一个改动**：甲案（`Governance` 加 `origins: BTreeMap<RunId, BlockedJob>`）一并解开两者——内存里的 `BlockedJob` 想带几个字段就带几个，不动账本 payload，也不需核黄金账本；乙案（改 `approval_requested` payload）则要把预算一并写进去。**未定事项**：甲案是进程内存，而重启后的 worker 从账本重建；若 origins 不重建，重启前提出、重启后才被批的项就接不上活。这一点在选定甲案前必须先用测试回答（现行全史扫描没有这个问题，这是它唯一的优点）。

## 8-24 一条效应先成为账本行，再成为这座城（整修卡 R2.10）

```rust
// crates/sprawling/src/effect.rs —— ARCHITECTURE.md §12 bin::effect，形状 2（值类型）
pub(crate) struct Line { who: String, addr: Address, kind: EventKind, data: Payload }

/// 一张桌子留下的全部效应：它们成为的行，以及行之后才允许发生的变化。
pub(crate) struct Landing { lines: Vec<Line>, then: Then }   // 两个字段都是私有的

pub(crate) enum Then { Nothing, Deliver(Vec<collab::Signal>), Hold(Vec<GoalEntry>),
                       Roadmap { path: PathBuf, text: String }, Shelf(Vec<Filing>) }

impl Landing {
    pub(crate) fn signals(Vec<SignalEffect>, room: &Address, who: &str) -> Result<Landing, AxError>;
    pub(crate) fn goals(Vec<GoalEffect>, room: &Address, who: &str) -> Result<Landing, AxError>;
    pub(crate) fn discards(Vec<Payload>, room: &Address, who: &str) -> Landing;
    pub(crate) fn shelf(Vec<ArchiveEffect>, write_root, building, at, room, who) -> Result<Landing, AxError>;
    /// 先走完每一行，再把变化交出去。这是 `Then` 唯一的出口。
    pub(crate) fn record(self, &mut impl FnMut(Line) -> Result<(), AxError>) -> Result<Then, AxError>;
}

/// 一跑对共享计划做的事。两种而无第三种：计划是整份写回去的。
pub(crate) enum Claims { Landed(Box<Landing>), Stale(Vec<u64>) }
impl Claims { pub(crate) fn of(&[ClaimEffect], on_disk: &str, text: String, path, room, who) -> Result<Claims, AxError>; }

// 装配层那一扇门（assembly）：五张桌子都走它，`Then` 的 match 穷尽
impl RunWorker { fn settle(&mut self, RunId, from: &Address, Mode, BudgetCap, Landing) -> Result<(), AxError>; }
```

**病灶**：`dispatch_in` 驱动之后有六段 `take_effects()`，每段都在做同一件事——把效应变成账本行，再把它变成状态。这条顺序在三份文件里各写过一次：`docs/glossary.md` 对 Ledger 的定义是「Every effect becomes an EventRecord first」，ARCHITECTURE.md §5 步 4 是「that ordering is the design's load-bearing rule, not a logging preference」，signal 那段自己的注释是「Recorded, then delivered. The queue may only change as a consequence of a line the history already has」。**六段里有两段是反的**：

```rust
write_plan(&plan_path, &text)?;                            // 先改共享计划
for effect in &claim_effects { self.record_for(…)?; }      // 后落账

let entry = city::file_archive(…)?;                        // 先上书架
self.record_for(…, EventKind::AssetArchived, …)?;          // 后落账
```

第二段的注释与它自己的代码相反：「Filed after the drive, like every other effect, **so nothing is on the shelf that the history does not already carry**」。按现行顺序，落账失败就在架上留下一条历史没有的记录，那句话就是假的。计划那一段更重：`roadmap_claimed` 是 `memory::hot` 与 `memory::projection` 判断谁拿着哪一行的依据，写进了文件而没落账的 claim 是一行看上去有人占着、历史里却无人占着的行。

**现形**：新模块 `bin::effect`。它不是把那五段搬个地方，而是把「先后」从人的纪律换成类型的性质：`Then` 只能从 `Landing::record` 里拿到，而 `record` 先把所有行送进去才返回它。要把顺序写反，得先拿到一个拿不到的值。

- **批而不是逐条**：一张桌子的行全部落完，才轮到它的变化。这改变了 signal 一支的交错方式（原先是 A 落账、A 投递、B 落账…），**但不改变账本字节**：`deliver` 与 `knock` 都不写账（`knock` 只往 `self.knocks` 推一条，由 drive 之后的 `answer_knocks` 统一开跑），所以 `signal_enqueued` 之间的先后原样。
- **计划那一支是全有全无的**，因此它自己一个穷尽枚举 `Claims`：任一条效应对不上盘上的那份，就一行不写、一行不落，只把动过的行号报给人——这是 R2.06 定下的形制，本卡只把它从 `dispatch_in` 里搬出来并把写盘移到落账之后。
- **`city::archive` 因此拆成两步**（详见 city-SPEC §8-9）：账本行要的 `kind`／`day`／`subject` 全是入参的函数，不需要先写盘就能算出来。不把 `day_of` 搬到装配层算一遍，是因为那会是「一条归档记录长什么样」的第二个权威。
- **`raised`（待批项）不进本模块**：它不是桌子交出来的效应，而是驱动期间被暂存的项，并且在落账前还要受 `tainted_arrival` 改写。它本来就是先落账后改状态的。

**pr 那两支不是同一类，本卡不动，理由记在这里以免下一个人重新查一遍**：

- `PrEffect::Opened` 里的 `wave_pre` 先于 `pr_opened` 落账，**但它不是「先动世界」**。它铸出的是那条账本行所指向的对象，与 `run_started` 之前那句 `self.cas.put(brief…)` 同形：没有任何记录指向的 git commit 不改变任何人读到的东西。交接件把它列为缺陷，我核完否定了。
- `PrEffect::Merged` 里的 `trees.merge` 确实先于 `pr_merged` 落账，而且它真的改变大家读到的干线。**先落账在这里更坏**：`merge` 有一条可达的失败臂 `MergeStale`（分支后干线又动了），先落账就是把一句谎写进历史里的可达路径，而不只是崩溃时的撕裂。要两边都对，`memory::Worktrees` 得先能回答「这一合并会落在哪个 commit」（它就是分支尖，`merge` 今天返回的也正是 `theirs.id()`）且能先验干线。那是另一张卡，它自己的红在 `MergeStale` 那一臂上。

**红**：`what_a_run_changes_is_changed_after_the_line_that_announces_it`。一跑归档一条决定、又从共享计划里拿一行；`RunWorker::observe` 的 sink **在一行耐久之后才跑**（`memory::jsonl` 自说：「runs on the appending thread after durability」），所以它正是「先」唯一看得见的位置。断言：`asset_archived` 落时书架上还没有它，`roadmap_claimed` 落时盘上的那一行还没被拿走；跑完两者都在位（只是排了序，不是丢了）。本卡之前两条断言各自撞红。

**影面**：`city` 公开面换一项、增一项（`file_archive` 改签名，新增 `archive_entry`），基线与 city-SPEC 同提交更新；`sprawling` 公开面不变（`effect` 是 `mod`，不是 `pub mod`）。

**尺寸**：`dispatch_in` 1069 → 983 行。搬走的结结实实是五段共 ≈150 行，其中 60 行以 `RunWorker::settle` 的形式回到本文件——那是五张桌子共用的那一扇门，不是 `dispatch_in` 的一段。**尺寸门要等这个数字降到门限以下才能开，本卡只是第一刀**；剩下最大的两块是驱动块（≈150）与目录及工具准入（≈120）。

## 8-25 一个答复接上的活，不靠重读全部历史找到，也不丢掉它的天花板（整修卡 R2.11）

```rust
// Governance —— 现在是 RunWorker 的一个字段，而不是四个散字段加一份重写
struct Governance {
    pending: BTreeMap<String, ApprovalItem>, autonomy: Autonomy,
    granted: Vec<ClusterKey>, halted: BTreeSet<String>,
    sent: BTreeMap<RunId, Sent>,          // 从 run_started 折；task、goal、budget
    origins: BTreeMap<String, BlockedJob>, // 从 approval_requested 折；答复时 O(log n)
}
impl Governance {
    fn sent(&mut self, RunId, task: &str, goal: &str, BudgetCap);   // 两个调用方，一个形状
    fn absorb(&mut self, EventKind, RunId, Option<&Address>, &Payload);
}
struct BlockedJob { addr: Address, task: String, goal: String, budget: BudgetCap }
```

**三个病灶，一个改动**（交接件第 4 项与§8-23 留下的那一半）：

1. **`blocked_job` 扫全史**。每次审批应答都 `verify_ledger_dir` 一遍再解析两遍，只为找 `(addr, task, goal)`，随历史线性增长。（量级取自 R2.02 在本机留下的同类读数：`verify_ledger_dir` 约 215k 记录/秒，于是 50k 的历史光验链就是百毫秒量级；本卡没有重测。）
2. **天花板归零**。`fn dispatch` 写死 `BudgetCap::default()`，而它正是审批应答后续活走的那条路。一跑带着天花板派出、因待批停下、被批准后续上的那一跑，向模型报 `0 usd_micros, 0 tokens`。
3. **`self.pending.remove(item)` 先于落账**，与 `set_admission` 相反，且是冗余的——`record → govern(ApprovalResolved)` 本就移除它。`self.granted.push(…)` 同理。

**为什么三件一起改**：它们是同一个结构问题的三个面。治理状态本来有两份实现：`Governance::absorb`（重启折）与 `RunWorker::govern` 加上 `set_admission`／`answer_approval` 里直改字段的几行（活折）。R2.02 测过两者不漂移，但那只是当时恰好相等；**再加一份 origins 折就是第三份**。本卡把四个散字段换成 `RunWorker.governance`，`govern` 就是 `absorb`，于是新的两张表只有一个折法。

**选甲而不选乙，理由比交接件写的强**。乙案是让 `approval_requested` 的 payload 自述所阻之活；但那份 payload 就是 `ApprovalItem` 本体，改它得改 `kernel::ApprovalItem` 的公开面与每一个构造点。更重要的是：**账本已经说得出一项是哪一跑提的**（envelope 的 `run`），它没说的是那一跑被派去做什么、在什么天花板下。那是 `run_started` 的事，不是每一项待批的事。

**交接件那个未决问题，用测试回答了**。它问：甲案是进程内存，重启后的 worker 从账本重建，那「重启前提出、重启后才批」的项接不接得上活？答：接得上，因为 `origins` 就在 `Governance` 里，而 `Standing::fold` 对每一行调的正是 `absorb`——与 `pending` 同一折、同一遍。`what_a_worker_holds_is_what_a_restart_rebuilds` 本卡增一条断言盯住它。

**不裁剪 `sent`，写明代价**。每跑一条（两个短字串加 16 字节），与 `memory::HotView` 同一增长级。**不能按 `RunFrozen` 裁**：`freeze` 在 drive 内落账，而装配层的待批项清扫在 drive 之后，账本顺序是 `RunStarted … RunFrozen … ApprovalRequested`，按 freeze 裁会先删掉待用条目。`origins` 则在 `ApprovalResolved` 上裁，因为答过的项不再阻着任何东西。

**读在落账之前，派活在落账之后**：`answer_approval` 先取一份 `origins`（读，不是变化），再落 `approval_resolved`（它自身就是关闭动作，`absorb` 随之丢掉 pending 与 origin），最后才派活。与 §8-24 同一条规矩。

**红（两条）**：

- `work_resumed_by_an_answer_is_done_under_the_ceiling_that_sent_it`：以 `BudgetCap { usd: 250_000, tokens: 4_000 }` 派一跑，它读一次 `status`（对照组），然后提一项待批；批准后续上的那一跑再读一次。**两跑同地址**，所以判据不是 `addr:` 而是该地址上读到的天花板去重后的**集合**：本卡之前是两个值（`0 …` 与 `250000 …`），之后是一个。数请求体不能作判据（§8-23 已记）。
- `what_a_worker_holds_is_what_a_restart_rebuilds` 增一条：活 worker 的 `origins` 与 `Standing::fold` 重建的逐项相等。本卡之前 `origins` 不存在，是编译红。

**性能以结构收口而不以计时收口**：一次审批应答从「验链一遍加解析两遍全史」变为一次 `BTreeMap` 查找；`blocked_job` 连同它的两个循环一并删除，因此这不是一个快了多少的问题——那条路径不存在了。

**影面**：`runtime` 公开面增一字段（`RunPlan.budget`），基线与 runtime-SPEC 同提交；`RunPlan` 的三个构造点（assembly、citysim、runtime 集成测）各加一行；`fixtures/golden-p0` 重生。

## 8-26 读不到一份文件不等于那份文件写错了（整修卡 R2.12）

```rust
fn city_segment(city_root: &Path) -> Result<Vec<u8>, AxError>;  // NotFound → 内置副本；其余 → Err
```

R2.05（交接件）与 R2.06（计划）定下的形制是：**「还没有」答默认值，「读不了」带着路径上报**。本卡收尾同族剩下的两处。

**一、楼页把「读不开」报成「表写错了」**。交接件把它记为「读不到只是页面少一块，不会变成误报」——**我核完否定了这个判断**。`read_building` 把读失败抹成空串，而 `check_roadmap_shape("")` 并不返回空结果：`header_seen` 为假使它推出 `Malformed { problems: ["no four-column table found"] }`。于是页面向人断言一件它无从得知的事：那张表的形状不对。人于是去修表格，而要修的是一个打不开的文件。

**现形**：改走 `city::roadmap`（R2.06 立的那扇门），读失败时**把失败本身放进 `problems`**——那正是这个字段的用途，也是页面已经会画的东西。`read_building` 不改返回类型：`None` 的意思是「没这栋楼」，把「计划读不了」塑成那个形状会让一栋存在的楼从城里消失。

**二、关城时把零字节当成城的规范**。`close_city` 的 `std::fs::read(&city_file).unwrap_or_default()` 使 must-read 指向空字节的 CAS 哈希：下一任被告知「先读这份」，读到的是什么都没有。

**现形**：不新建读法，改用同文件已有的 `city_segment`——「这座城的规范是什么」应当只有一个答案，而 prefix 装配已经在问同一个问题。同时把 `city_segment` 自己改成同一形制：它原本的 `unwrap_or_else(|_| CITY_MD)` 注释自述为「falling back to the built-in copy **when a city predates it**」，而那只描述了 `NotFound`；其余失败下它静默地拿内置副本冗作人编过的那份，而两份可以完全不同。修后：`NotFound` 仍答内置副本（那是已记录的契约），其余一律带路径上报，于是一跑在读不了的城规范下开跑这件事也一并没了。

**关城于是会失败，这是有意的**。一次说不出下一任该读什么的关闭不是一次有序关闭；`serve` 的循环已经写着 `eprintln!("the city could not write its handoff: {err}")`，于是人在终端上拿到路径与修法，而不是一条指向空白的交接件。

**红（两条，各咬一处）**：把 `Roadmap.md`／`City.md` 各做成**同名目录**（R2.05／R2.06 用过的手法，不碰权限，在 Windows 上稳定）。一：楼页的 `problems` 必须点名 `Roadmap.md`——本卡之前它说的是 `no four-column table found`。二：`close_city` 必须以点名 `City.md` 的错误拒绝——本卡之前它返回 `Ok` 并写下一条指向空字节的 must-read。

## 8-27 一次登记喂到两处，于是只写一遍（整修卡 R2.13）

**病灶不是缺陷，是两个权威**。`dispatch_in` 里目录准入与工作台注册是两份各十三行的名单，而同一段的注释自述「one registration feeds both」。两份名单今天相等，但相等是人维护出来的：只上工作台的工具是没人能叫的工具，只上目录的工具是告诉了模型、叫下去却不存在的工具。

**现形**：一个 `Vec<Box<dyn kernel::Tool>>`，一个循环里先 `admit_tool(tool.meta())` 再 `bench.register(tool)`。順序取**目录的**那一份：`Catalog::render` 按准入顺序把工具摆在模型面前，而 resident 段是要算哈希的，所以这个顺序是缓存面的一部分。十三件的次序逐字照旧代码排（archive、exec、claim、edit、status、signal、goal、pr、delegate、workshop、rules、neighbours、read，然后 MCP），故字节不变。

**它以什么收口**（照 §8-17 的写法）：**没有可咬的红**，因为两份名单今天并未漂移——我逐项对过，十三对十三。一条「两集合相等」的断言今天就绿，而且改完之后它恒绿（不可能不相等），那不是测试而是装饰。收口在于：变化后两份名单不可能不相等，且 141 条现有测试（包括多条断言工具名与 prefix 内容的）全绿。**不为了凑一个红而补一条前后都绿的测试。**

**尺寸不是本卡的理由**：`dispatch_in` 983 → 977。五十行准入换成四十五行名单加循环，净值接近零；换来的是一个权威而不是两个。

**本卡推翻的一个假设（写在这里以免重走）**：我判断 `invoke` 里 `match bench.invoke(…)` 的 `_ =>` 臂是死代码——`BenchOutcome` 四个变体已全部列出，且 ARCHITECTURE.md §7 的纪律是「新增一种答案而不回答它就不编译」。删掉它即得 `E0004`：`BenchOutcome` 带 `#[non_exhaustive]`，而本 crate 在它定义的 crate 之外，因此永远无法穷尽匹配。那一臂因此保留，并注明它为何不可达。

——**留给下一个人的问题**：`runtime` 不发布（ARCHITECTURE.md §3：「nothing here is published」），而 `#[non_exhaustive]` 是为 crate 外的第三方准备的。在一个工作区内部的判定输出上用它，换来的是每一个下游 match 都得写一个永不执行的分支，而代价正是 §7 想要的那个编译期穷尽性。runtime-SPEC 第 123 行已写下一条相关规则（「14.3 的 non_exhaustive 规则辖 wire 冻结枚举，不辖判定输出」），而 `BenchOutcome` 正是一个判定输出。**这一条看上去是规则与实现不符，但改它动的是 `runtime` 公开面且没有红，故本卡不动，只点名。**

## 8-28 一次调用的键，只有一份读法（整修卡 R2.15）

```rust
// dispatch_in 驱动块内：六行手写的动作字节换成一个问句
let key = kernel::IdemKey::derive(&run_id, kernel::Seq::new(at), &call.action()?);
```

**本卡不修 `bin::assembly` 的缺陷，因为这里没有缺陷**。被修的是 citysim（citysim-SPEC §8-4）；本文件变的是「谁来回答动作字节」。原先这六行把 name 与 `serde_json::to_string(&call.args)` 拼起来，是全库两份实现中对的那一份；对的那一份待在装配层，正是另一份能静静漂走的原因。`kernel-SPEC §8-6` 早写着这条规则「属 S2 工具面」，而它一处也不在那里。现在它在（`ToolCall::action`，kernel-SPEC §8-23），本文件改为问它。

**字节逐字不变**：`action()` 内部就是搬过去的同一句（name 字节接 args 的 JSON 字节），位次仍是本地 `placed` 计数器。唯一的行为差异是 `unwrap_or_default()` 换成 `?`：一个序列化失败以前产空串（于是两次参数不同的调用得同一把键），现在上报。`Payload` 拒浮点且键恒为字符串，故这一臂今天不可达。

**本卡推翻的一个假设（写在这里以免重走）**：我先判「让 `ToolBench` 自己持 run 与位次、`invoke` 内部铸键」是更好的形——传钟进来这件事就没有参数可传。核完否定：`ToolBench::seen` 恒从空集起，且键在过门之后才记入（runtime-SPEC §532），所以一个恒递增的内部位次会让键在一次驱动内永不重复，`BenchOutcome::Duplicate` 随之变成**任何门都达不到的变体**，`turn.rs` 那条 `dedup_runs_before_the_side_effect`（同键调两次、断言文件未再变）连同它守的不变量一起写不出来。**把一个可测的防御换成不可测的死代码，不是加固。** 位次因此留在调用方。

**顺手记下、本卡不动的一件事**：dedup 是一道**今天接不到任何东西的防御**。`kernel::idem` 自述它存在是为了「resume 与 replay 重派出同一把键」的双付防御，而 `seen` 从不从历史播种，`sprawling resume` 也不重跑一跑（ARCH §5 末：它只验链、把丢了结果的调用关成 unknown、并报告等人的事）。本卡之后，`Duplicate` 在两个驱动器里都不会再出现，而这是**对的**：它本就是重放路径上的结果。要让它真正接上，得让 `seen` 从账本重建——那是另一张卡，它自己的红在「重建后的 worker 不会把已经付过的钱再付一遍」上。

## 8-29 行没落下，城就没动（整修卡 R2.17）

```rust
impl RunWorker {
    pub fn new(city_root, vault, log) -> Result<Self, AxError>;              // = open ➕ over
    pub(crate) fn over(city_root, vault, log, ledger: JsonlLedger) -> Result<Self, AxError>;
}
```

这是 §8-24（R2.10）那条性质的另一半。R2.10 把「行在变化之前」变成了类型的性质（`Then` 只能从 `Landing::record` 里拿到）；这里问的是「**行没落下，城就没动**」。

**选甲而不选乙，而且交接件对甲的反对意见不成立**。交接件写着甲案（`RunWorker::over`）「只有一个生产调用方，近乎为测试拓宽」，而乙案（倒置 `kernel::Ledger`）才是 ARCH 点名的那类动作。核完两头都不对：

- **甲不是测试拓宽，是 ARCH §3 自己提的那条批评**。§3 末段写着 `RunWorker`「builds its model adapter **instead of receiving one**」，并把它列为 V6 停在装配层下方的原因。同一句逐字适用于账本：一个自己 `open` 账本的 worker 同样无法被驱动到第二份实现上。把「账本从哪来」从构造子里取出去，是把一个不属于它的决定交回给调用方。
- **乙今天买不起**。`RunWorker` 对账本用的不只 `append`，还有 `position()`（两处）与 `observe()`。把 `observe` 推上 `kernel::Ledger` 等于让最内层去定义什么是「耐久后通知」——那是持久化适配器的事，不是「一个 Ledger 是什么」的事；代价是全库 **七个 `impl Ledger`** 各长出一个它们不需要的方法，加 conformance 套件。而本卡根本不需要第二个类型：两条路上都是具体的 `JsonlLedger`，**不同的是它下面的 `Vfs`**。既然缝不必动，就不动。
- 丙（只在 `memory` 内写红）**已经存在**：`power_cut_matrix_over_every_op_keeps_acknowledged_waves`。它证的是账本自己的耐久契约，不是装配层的不变量，所以它不替代本卡。

**它以什么收口：一张没有红的卡，照 §8-17／§8-27 的写法说清楚**。`a_line_the_history_refused_is_a_change_the_city_never_made` **首跑即绿**，因为这条性质 R2.10 已经用类型持住了：`record` 遇拒即 `?` 返回，`Then` 随之丢弃，改变无从发生。**我没有补一条前后都绿的测试就算完事**：把 `Landing::record` 的 `append(line)?` 改成 `let _ = append(line);` 后重跑，它当场红，且红在实质那条断言上——盘上的计划被写成了 `| 1 | wire the kiln | In progress |  |`，而宣布它的那一行从未落地。恢复后又绿。这条测试因此是一张网，不是一条红，而它能咬是量出来的不是声明出来的。

**测试里两个世界各归各位**：账本在 `FaultFs` 的内存平面上，城的文件（`Roadmap.md`）在真盘上。这正是要问的形状：被断言的东西是一份人事后真能去打开的文件。`Standing::fold` 仍读真目录（那是「城到目前为止知道什么」），而本跑新落的行进虚拟账本——两者不相干，因为断言不靠账本内容，只靠盘上那份计划。

**影面**：`memory` 公开面在 `fault` 下增 `open_faulty`、`FaultPlan` 增一字段（memory-SPEC §8-2 同提交）；`sprawling` 公开面**不变**（`over` 是 `pub(crate)`）；`crates/sprawling/Cargo.toml` 的 dev-dependencies 打开 `memory/fault`，发行构建不含它。

## 8-30 合并也排到它那条行后面（整修卡 R2.18）

§8-24 把五张桌子搬进 `bin::effect` 时，把 `PrEffect::Merged` 留在原地，理由写得很清楚：`trees.merge` 确实先动世界，但「先落账在这里更坏」——`merge` 有一条可达的失败臂 `MergeStale`，先落账就是把一句谎写进历史里的可达路径。它同时写下了解法：`memory::Worktrees` 得先能答「这一合并会落在哪个 commit」且能先验干线。本卡做的就是那一条（memory-SPEC §8-2），于是两头不再互斥：

```rust
let planned = trees.plan_merge(&name)?;    // 全部拒绝在此，世界未动
record_for(…, EventKind::PrMerged, … planned.commit() …)?;   // 行
planned.apply()?;                           // 才是变化
```

于是：一个会被拒的合并永远不会先得到一条行（`MergeStale` 早于落账）；一条没落下的行也永远不会已经改了干线（`apply` 需要一个只能从 `plan_merge` 拿到的值，而行写在它之前）。

**红**：`a_merge_the_history_refused_leaves_the_building_where_it_was`。一个 `review: true` 的楼，一跑改文并提交请求，第二跑去检——而第二跑的账本是 `open_faulty`（§8-29 的工具）且 `cut_on_write: Some("pr_merged")`。断言：楼里那份文件仍是 `before`。**本卡之前它是 `after`**：干线已经移了，而宣布它的那一行从未落地——一座楼站在它自己的历史说从来没有并入过的工作上。我把次序改回去跑了一遍看它撞红，再改回来。

**影面**：`memory` 公开面去 `Worktrees::merge`、增 `plan_merge` 与 `PlannedMerge`（基线与 memory-SPEC 同提交）；四个读写方全部迁完后旧入口删除，不留适配。`sprawling` 公开面不变。

## 8-31 dispatch_in 向 ARCH §5 的十二步靠拢（整修卡 R2.19，逐刀）

**目标不是「把某一段搬走」，是「让 `dispatch_in` 成为 ARCHITECTURE.md §5 已经写好的那个序列」**。两者的区别是形状问题的生死：按行号切出来的一块叫不出 §9 的名字，而一个相位叫得出来——§5 已经给了它名字。

**第一刀：驱动（§5 步 7–11）。**

```rust
struct Driven {
    outcome: Result<runtime::Run<runtime::run::Frozen>, AxError>,   // 仍是 Result：跑败也要结桌子
    fenced: Vec<String>, ran: (u32, u32), raised: Vec<ApprovalItem>,
}
impl RunWorker {
    fn drive_dispatch(&mut self, plan, handoff, adapter, bench, signals,
                      write_root, fence_scope, who, run_id) -> Result<Driven, AxError>;
}
```

三个钩子住在一起，理由不是它们相邻，而是**它们是唯一在驱动器持有账本期间碰账本的代码**（`invoke` 里那句自述：「the ledger is the driver's for the length of the run」）。它们收集的三样东西也只在那段时间里可写，所以一并作为 `Driven` 返回，而不是留四个 `Rc<RefCell<…>>` 让调用方自己保持同步——四个单元格是四个可以忘记读的东西，一个值不是。

`outcome` 刻意仍是 `Result` 而不在方法里 `?`：一跑失败了它的桌子照样要结，而结桌子正是把它最后几行放上历史的动作。把它提到方法边界上会静静跳过它们。

**尺寸**：`dispatch_in` 975 → **833**；`drive_dispatch` 171。尺寸不是本刀的理由（照 §8-27 的写法），但它是 R2.20 尺寸门的前提，而那道门的门限是量出来的 200。

**它以什么收口**：纯结构，无可咬的红——行为逐字不变（钩子体原样搬迁，`fence_scope` 由计算改为传入）。143 条 `sprawling` 测试全绿，其中包括直接盯驱动行为的 §8-24／§8-29／§8-30 三条。

### 剩下的五刀（本会话未完，按 AGENTS.md 「把剩下的写进它所属的 SPEC 节」）

`dispatch_in` 今为 **833** 行（@3611），相位实测如下。目标 <200；每刀都是同一个形制：相位成为 `RunWorker` 的一个方法，多个活值归并为一个归位值类型（如 `Driven`），而不是一排得保持同步的局部变量。

已切五刀（R2.19a–d），**975 → 548**，产出的方法均在阀值内：`drive_dispatch` 171、`settle_desks` 124、`settle_requests` 122、`conclude` 104、`admit_reading_room` 32。两个归位值类型：`Driven`（驱动期间写、驱动之后读的四样东西）与 `Desks`（一起出借、一起结算的四张桌子加计划路径）。

**剩下三刀**（目标 <200，预计落在 ~180）：

| 相位（§5 步） | 长度 | 归位值 |
|---|---|---|
| 规则／配置／选型／身份／租约 | ≈62 | 一个 `Site` 值（addr、building、rules、config、who、write_root、branch） |
| 五张 desk 的构造 | ≈75 | 返 `Desks`（类型已存在，R2.19c 建） |
| catalog＋十三件工具＋bench（步 6） | ≈175 | 一个 `Workbench`（catalog、bench、delegates、workshop） |
| prefix＋RunPlan＋handoff（步 5） | ≈90 | 返 `(RunPlan, Handoff)` |

工具那一块原为 214 行，本卡先把阅览室（`admit_reading_room`）切出去，余下 ≈175 才能装进一个合格方法。**这正是阀值取 200 的一个副作用**：它不允许把一堆东西搬到另一处冒充分解。

**R2.20 尺寸门须等这五刀完成**：纪律是「不为通过而放宽门」，所以门不能先落地再给自己开例外。门限与单位由全库测量定下，数字就写在这里：单位是**生产函数**（以首个 `#[cfg(test)]` 截断），门限 **200 行**。依据：1646 个生产函数中位数 9、p90 为 37、p99 为 114；超过 200 的只有六个，而其中五个是数据与标记（`web::lang::phrase` 是译文表，属 §9 形状 6；`Settings`／`CityView`／`BuildingView`／`Root` 是 Dioxus 组件，函数体即标记），故这两类需在门里声明为数据。排掉它们，全库超阀的生产函数只剩 `dispatch_in` 一个，第二名 `serve` 为 233——它也在网内，这是故意的，把门开到 240 去放它过就是为通过而放宽门。而按**文件**计不行：任何诚实阀值都会在四个 crate 里同时点燃八处（800 行阀 → 8 个文件），那是工程而不是一道门。提交须带 `Verdict: user-approved`，依据与 `a9b1522` 同：常驻指令即那条裁定。

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
