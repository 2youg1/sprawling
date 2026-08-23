# web-SPEC.md

> crate：`web`（lib，依赖 channels）。本 SPEC 先于代码存在；实现不多不少地遵守本文。
> 骨架：十七节；按模块分章、每章自足。
> Stage 4 十模块：app／socket／city_view（骨架）／progress／dashboard／live／approval／ledger_view／alert／theme。
> 本 crate 覆盖的语义：模块清单；多台机器一个界面；视觉语言与体验全章；前端框架选型；性能预算；依赖钉版表。
> 本 crate 编译到 `wasm32-unknown-unknown`，不入 `default-members`；产物由 `crates/sprawling/build.rs` 嵌入，交付物仍是单二进制。

## 1 需求拆解

| 卡 | 模块 | 一句话 |
|---|---|---|
| S4.01 | —— | 前端框架结论书：定下「Rust 编译到 WebAssembly」的具体方案，写明度量方法、败诉线与被否方案的理由｜**须 verdict** |
| S4.05 | `socket`＋`app` | WS 客户端（握手·重连·事件流入口，全 crate 唯一通信处）＋根组件与视图路由；视图＝纯函数(快照, 事件流)，不持业务状态 |
| S4.06 | `theme` | OKLCH 七项源头常量 → CSS 自定义属性的唯一生成处；去色＝一个色度系数置零；`xtask color` 消费本模块常量 |
| S4.07 | `progress`＋`approval` | progress bar 唯一渲染处（三态上色，三调用方共用）＋Approval Inbox 与 Recycle Bin |
| S4.08 | `dashboard`＋`live`＋`ledger_view`＋`alert` | CostView／Metrics 曲线｜会话直播｜Ledger 浏览（过滤·跳 CAS·导出）｜唯一 ALERT 产出处 |
| S4.05 | `city_view` | 等距画布骨架：接口定死，绘制层 P2 填充 |

## 2 验收标准

- **theme**：七项源头常量是全 crate 唯一颜色产地；`xtask color` 六断言（色相恒 264 或 84、灰阶 C=0.018 十一档、L∈[0.145, 0.930]、两彩色令牌取色度比例而非写死 C、progress 渐变两端同轴、无颜色字面量散落）在真常量上跑通。去色快照＝同一份样式把色度系数设为 0 重拍，界面语义仍完整。
- **socket**：握手 schema 哈希不配即拒连并显式报错（不静默降级）；断线重连以指数退避且不丢事件序（服务端按 `seq` 续传）。
- **app**：视图函数对同一 `(快照, 事件流)` 恒产同一 DOM 描述——同输入两次渲染的输出等值（Humble Object：难测的一端是 DOM 应用，厚的一端保持纯粹）。
- **city_view**：Stage 4 只验接口存在与画布挂载；确定性布局（Building 按 id 稳定哈希落位）与位图回归属 P2。
- **无头浏览器驱动入 CI**（S4.08）：普通界面走端到端与视觉回归；对比度在渲染后的页面上实测（明拒启动时计算）。
- **性能预算**（P0 只记录趋势）：前端产物传输量 ≤2MB 压缩后（字体分片不计入）；浏览器打开→可交互 ≤1.5s。

## 3 假设与歧义

- **前端只收 HTML，不带 Markdown 解析器**（B.7 `pulldown-cmark` 行）：服务端渲染完 HTML 下发，故本 crate 必须有一条「注入受信 HTML」的通路。该通路的信任来源是服务端渲染，不是模型输出——taint 的封锁在服务端完成，本 crate 不做第二道判定（两个权威即错）。
- ~~**city_view 画在画布上，不画成 DOM**：一千个 Resident 不做一千个节点。故框架选型只需服务九个 DOM 模块，第十个模块要的只是一块画布与 2D 上下文。~~ **F2.02 推翻了这一条**：这幅图从来不画 Resident，只画 Building，而一座城是几十栋楼；节点数论据够不到本场景，代价却是四件确定的损失（见 §8-28）。框架选型不受影响——被否的 egui 一档是因为画布 UI 没有 CSS 这一层（§8.5-6 第 6 条），而 SVG 恰好在 CSS 里。
- **字体不整份加载**：按字符区间分片，浏览器只取用到的那几片；字体文件作为静态资源嵌在二进制里。分片切割属 S4.08 之后的资源工程，不属框架选型。
- **`web` 的公开面对 `cargo public-api` 的口径**：本 crate 无下游消费者（拓扑末端），`apisync` 基线集是否纳入 `web` 待 S4.05 定；倾向纳入，理由是「公开项只降不升」对末端 crate 同样是有效的复杂度刹车。

## 4 现状分析

空壳 lib（`src/lib.rs` 仅 crate 文档）＋一张占位页 `assets/index.html`（Stage 0 落，558 字节，由 bin 的 `build.rs` 复制进 `OUT_DIR` 后 `include_bytes!`）。无既有公开面，api-baseline 自本期起算。占位页里的三个十六进制颜色（`#070A12`／`#C1C7D3`／`#848994`）是 G0／G9／G7 的 sRGB 参考值，`theme` 上线后必须由样式变量取代——否则 `xtask color` 的颜色字面量扫描会咬住它，这是设计意图不是缺陷。

## 5 权威信源

视觉语言全章（其权威表示是 `web::theme` 的 OKLCH 源头常量）；web 各模块的职责承诺（ARCHITECTURE.md §12）；多台机器一个界面；性能预算表（`xtask/budgets.toml`）；依赖钉版表的「Rust 到 WebAssembly 的前端框架 / wasm-bindgen / web-sys」行。选型证据的外部信源逐条列在 §8.5 结论书内，均带取证日期。

## 6 命名统一

ACCENT／ALERT／G0–G10／PROGRESS_DONE；Approval Inbox／Recycle Bin／Autonomy／Address／CostView／Metrics／Ledger／Locator／Run／Resident／Building（附录 A 概念名英文原词）。**去色**＝desaturation，机制是色度系数置零，不是第二套样式。**五区版面**＝顶栏／左导航／右状态／底 control surface／中央。

## 7 模块边界

```
socket（唯一通信处）──▶ 快照＋事件流 ──▶ app（视图路由，无业务状态）
                                          ├─ progress（唯一 progress bar 渲染处）
                                          ├─ approval ─┐
                                          ├─ dashboard ─┼─ 均消费 progress
                                          ├─ live ──────┘
                                          ├─ ledger_view
                                          ├─ city_view（SVG，F2.02 前为画布）
                                          └─ alert（唯一 ALERT 与浏览器通知产出处）
theme（OKLCH 源头常量 → CSS 自定义属性）── 全 crate 唯一颜色产地
```

**不做什么**：不解析 Markdown（服务端已渲染）；不做第二道 taint 判定；不持有业务状态（状态是服务端快照的投影）；不实现颜色空间转换（浏览器做色域映射，做得比任何内置近似更好）；不做去色开关（去色是命令行开关与快照测试的能力，设计明确减掉它）；不设 city view 图例；`web` 不声明任何 `pub` trait（不在 ARCHITECTURE.md §4 缝清单内）。

## 8 接口先行（按模块分章）

### 8-1 web::app（S4.05；形状 7 projection ＋ 形状 1 纯函数）

```rust
pub enum View { City, Live(RunId), Approvals, Dashboard, Ledger }   // 中央区路由
pub enum ProviderHealth { Unknown, Healthy, Degraded, Lost }
pub enum RunPhase { Running, AwaitingApproval, Frozen, Halted }
pub struct RunRow { addr, phase, steps_done, steps_planned, started_at_seq }
pub struct Snapshot { /* 字段全私有 */ }
impl Snapshot {
    pub fn apply(&mut self, &EventRecord) -> bool;   // 前进式；返回“是否真的动了”
    pub fn resume_from(&self) -> Option<Seq>;
    // 右状态常驻四项：city()／spent()／approvals_pending()／provider()
}
pub fn rebuild<'a>(impl IntoIterator<Item = &'a EventRecord>) -> Snapshot;
pub fn status_line(&Snapshot) -> [String; 4];
pub fn render_usd(UsdMicros) -> String;
#[component] pub fn Root(snapshot: Snapshot, view: View) -> Element;
```

**Snapshot 不是第二份历史**。它是 `memory::hot` 在浏览器里的同形物：可弃、可重建、前进式、幂等。重叠交付（seq ≤ 已应用）恒为空操作——这正是重连可以**少要一点**而不必算准切点的原因。

**未建模的 EventKind 跳过而不拒**。这不是 fail-closed 让步：fail-closed 管的是**会产生效果的判定**，而视图不产生效果。服务端领先一个版本时，界面应当对它看得懂的部分保持诚实，而不是整片空白。

**钱的渲染全程整数**（`render_usd`）。为了显示而转 `f64` 会把全库花力气避开的那一次舍入又请回来。

### 8-2 web::socket（S4.05；形状 1 判定机 ＋ 薄壳）

```rust
pub enum LinkState { Idle, Opening, Handshaking, Live { resume_from }, Backoff, Refused(Box<AxError>) }
pub enum LinkEvent { Opened, Received(Box<ServerFrame>), Closed, TransportFailed, WaitElapsed }
pub enum LinkAction { Nothing, OpenSocket, Send(Box<Hello>), Deliver(Box<EventRecord>), Answered(Box<Answer>), WaitMs(u64), Report(Box<AxError>) }
// Answered（P1.03）：Query 的答面回到发问的视图；握手期收到 Answer 与握手期收到 Event 同判——先答后迎不是本协议。
pub struct Link { /* state ＋ token ＋ consecutive_failures */ }
pub fn backoff_ms(attempt: u32) -> u64;      // 梯子表，总函数
```

三条决定：

1. **schema 不配是终态**。`Refused` 不重试。对一个说不上话的服务端旋转重试，等于用 progress bar 代替那一句能解决问题的话（刷新页面）。
2. **退避确定而无抖动**，理由与 `gateway::admission` 同：随机抖动是概率性的，同时醒来的多个客户端可以全部摇到低值。梯子末端**拉平不再增长**：合上笔记本的人重新打开时，界面应当在一分钟内活过来。
3. **重试计数住在 `Link` 而非 `LinkState::Backoff`**（S4.05 红转绿抳出的 bug）：每次重试都要经 `Opening` 回到 `Backoff`，计数器若住在相里就被自己的重试循环清零，**梯子永远停在第一档**。计数是链路的属性，不是某个瞬时相的属性。

### 8-3 与构建链的接口（S4.05 已实测）

`channels` 分出 `server` feature。理由是硬的：tokio 的 mio 编译不到 wasm32，而 `web → channels` 是 depmap 冻结边。`web` 取 `default-features = false`，只得到 wire 词汇，不拖一个 TCP 栈进浏览器。**被否**：把 wire 拆成第六个 crate——那要改 §2 冻结拓扑，而 feature 边界已足以表达这个分割。

`web` 取 `crate-type = ["cdylib", "rlib"]`：cdylib 供 wasm-bindgen，rlib 供 host 测试链接。

**实测（2026-08-21）**：`cargo build --release --target wasm32-unknown-unknown` → 1,362,777 字节；过 `wasm-bindgen --target web` → `web_bg.wasm` 423,865 字节 ＋ `web.js` 56,175 字节；wasm **gzip 后 154,684 字节**。预算「前端产物传输量 ≤ 2MB 压缩后」有了首个真实读数，余量约 13 倍。

### 8-4 web::theme（S4.06；形状 6 数据面 ＋ 一个解算器）

```rust
pub const HUE_AXIS: u16 = 264;
pub const HUE_ALERT: u16 = (HUE_AXIS + 180) % 360;   // 派生，不是选的
pub const GRAY_CHROMA: u16 = 18;
pub const L_FLOOR: u16 = 145;  pub const L_CEILING: u16 = 930;
pub const ACCENT_CHROMA_PERCENT: u16 = 90;  pub const ALERT_CHROMA_PERCENT: u16 = 55;
pub const GRAY_RAMP: [(&str, u16); 11];              // 名，明度‰
pub const COLOUR_TOKENS: [(&str, u16, u16, u16); 4]; // 名，明度‰，色相，**比例%**
pub const PROGRESS_DONE: (&str, &str);  pub const CHROMA_COEFFICIENT: &str = "--chroma";
pub fn custom_properties() -> String;               // 全库唯一颜色产地
pub fn gamut_chroma_ceiling(l: u16, h: u16) -> u16; // 二分搜索
pub fn resolved_chroma(l: u16, h: u16, percent: u16) -> u16;
pub fn per_mille(u16) -> String;                    // 145 → "0.145"，精确
```

**值以每千分整数存**。`oklch()` 要分数，由整数格式化产出，两次构建不会因浮点而差，且门比的是整数。

**彩色令牌只写比例不写 chroma**。解比例需知 sRGB 色域上限，故有唯一一个浮点函数 `in_gamut`，**它只返 yes/no**，上层二分搜索与其余一切保持整数。实测佐证：ACCENT 解得 0.151，与令牌表**逐位相同**；ALERT 解得 0.057 对 0.058（差 1‰，整数舍入）。hover 变体沿用基色比例（反推得 91%／56%），故全库**恰两个比例**，「两个比例分立不合用」因此可机检。

**去色＝`--chroma` 置零**，测试断言它恰好覆盖全部彩色令牌且不碰灰阶（灰阶的 chroma 是使它在轴上，不是使它有色）。

**两份文档之间的一处分歧（门所发现，已裁定）**：一处正文写「最亮 L=0.930」，同章表却把 ALERT_HOVER 放在 0.945。两者同时发布，故取能使表合法的读法：L_FLOOR／L_CEILING 界定**灰阶信息面**，交互变体按设计在其上；无例外的规则是「不得纯黑纯白」。

### 8-5 web::progress＋web::approval（S4.07；形状 1）

```rust
pub enum BarState { Running, Done, Blocked }
pub struct Bar { state, filled: Option<u16>, label: String }
pub fn bar(&Progress, blocked: bool) -> Bar;
pub fn distinguishable_without_colour() -> bool;    // A17 末条写成函数
pub struct Cluster { summary, members: Vec<ApprovalItem>, answer_individually: bool }
pub fn inbox(Vec<ApprovalItem>) -> Vec<Cluster>;
pub enum ReturnPath { FromCheckpoint(_), FromStore(_), Rebuild(_), Undescribed }
pub fn recycle_bin(Vec<BinRow>) -> Vec<BinRow>;
```

**A17 的类型半不在本模块**：`UnplannedProgress` 无 `ratio` 方法，故本模块**画不出百分比不是因为守规矩，而是无从下手**。改画步数＋预算占用。

**三态去色可分辨写成函数而非截图**：两两之间明度或形状必有一处不同。blocked 与 done 明度接近，故 blocked 独携一道竖纹。

**blocked 压过 done**：纸面走完但仍卡在人身上的行，画成已完成就把界面存在的理由抹掉了。

**tainted item 以自身 id 为分组键**，故其组恒为一元组——C15「一次应答不得覆盖没读过的问题」由构造保证。`Restoration` 的通配臂按 fail-closed 补成 `Undescribed`：行照显，但**不编造一个自己兑现不了的恢复动作**。

### 8-6 dashboard／alert／ledger_view／live／city_view（S4.08；形状 1＋6）

**dashboard**——五维即成本归因的五维（与 `memory::attribution` 对同一权威额）。占比按**权威总额**算，不按行和归一：未归因余额是诚实的（A20 保留它），归一会把它藏掉。序列靠线宽×线型区分，**第五条序列拒绘而非复用图案**（`drawable()`）。本页恒不给建议，只把事实排序。

**alert**——ALERT 与浏览器通知同一道闸，因为它们是同一个判断：「这需要一个人」。同一 key 只打扰一次，`clear` 后再来才算新事实（否则冻结一小时的 Run 会每秒通知一次）。只有 `AwaitingApproval` 与 `RunFrozen` 会打断人：教会人忽略通知，代价是这两类也被忽略。顶栏只答**有没有**不答**几个**（未读计数是 17.4 明拒的）。

**ledger_view**——它存在的理由是设计对自己的翻案：live 与 dashboard 都按 Run 组织，答不了「这座城到底发生过什么」。过滤**恒报「略过了多少条」**——静默省略的窗口比没有窗口更糟。导出首行自称 `a filtered view, not the Ledger`。

**live**——窗口有界（老行掉出视图不掉出历史，这正是 ledger_view 存在的理由）；跟随是**粘性**的——滚回去的读者是在读东西，下一条事件把他拽走就是抢走它。

**city_view**——S4 只建几何，P2.10 填绘制层（见 §8-12），S4 定下的签名一个未动。**投影与反投影共用一套几何**（两套几何＝两个权威，而漂开的总是没人看的那一个）。落位是 id 的纯函数、画家序全序——位图回归的确定性前提在此兑现。

**施工中抳出的真 bug**：2:1 投影在奇数瓦片宽下 `tile_height*2 ≠ tile_width`；从瓦片到像素偏移中间有两次折半，故瓦片宽必须取 4 的倍数，否则命中测试不再是绘制的逆。

### 8-12 P2.10 绘制层：city_view 的形状表

```rust
pub struct Face { pub id: String, pub token: &'static str, pub points: [(i32, i32); 4] }
pub struct DisplayList { pub camera: Camera, pub faces: Vec<Face> }
pub fn faces_of(camera: &Camera, prism: &Prism, selected: bool) -> [Face; 3];  // 唯一「棱柱变几何」处
pub fn draw(camera: &Camera, prisms: Vec<Prism>, selected: Option<&str>) -> DisplayList;
pub fn pick(camera: &Camera, prisms: Vec<Prism>, x: i32, y: i32) -> Option<String>;
```

- **交付的是形状表而不是一串画布调用**：浏览器把它变成调用，无头运行把同一份表变成位图，两者因此不会漂开。这也是「位图对比比截图更适合确定性回归」在接口层的形态。
- **命中测试与绘制读同一个 `faces_of`**：`pick` 逆画家序取第一个命中者——两个棱柱重叠处，人点的是看得见的那个。
- **整数几何**：无浮点；点在凸四边形内用叉积同号判定，落在边上算命中，于是相邻两面之间没有点得进去的缝。
- **一层楼抬半个瓦片高**：三层比两层在每个 zoom stop 上都看得出来，而相机拟合时已为塔留出余量（`fit` 的竖向用 `n+3`）。

### 8-11 P1.05 接线：浏览器半边

```rust
pub fn read_frame(text: &str) -> LinkEvent;                     // 解析不出＝Closed，不是丢帧
#[cfg(wasm32)] pub fn socket_url() -> Option<String>;            // 由页面自身 origin 推出
#[cfg(wasm32)] pub fn open(url, on_event: impl FnMut(LinkEvent) + 'static) -> Result<WebSocket, AxError>;
#[cfg(wasm32)] pub fn send(socket: &WebSocket, frame: &ClientFrame) -> Result<(), AxError>;
#[component] pub fn App() -> Element;                            // 持 Snapshot 信号，渲染 Root
```

- **地址由页面自身推出，不可配置**：客户端由它所服务的城发出，故 `ws(s)://<host>/ws` 是唯一可能的对端。一个可配置端点会让「这个页面在跟谁说话」成为用户要回答的问题。
- **外壳零判定**：`open` 只把浏览器回调翻成 `LinkEvent`；重连时机归调用方（定时器属于持有帧循环的运行时，不属于传输层）。监听器在机器执行中途触发时**丢弃该事件而非重入**——socket 会再报一次，而半应用的相变不会自己恢复。
- **只有 `Deliver` 动业务状态**：答面、拒绝、退避梯子都不是历史，故都不写 Snapshot。这条在 `App::connect` 的 match 上是穷尽的。
- **本卡的验证面**：跨 crate 契约测试 `crates/web/tests/server_contract.rs`——用服务端自己的类型造帧、按 socket 的方式序列化、按客户端的方式读回，再喂进 `Link`＋`Snapshot`。它抓的正是无头浏览器会抓而单元测试抓不到的那一类：两端各自自洽却对不上。浏览器内的真实往返仍待驱动环境。

### 8-12 web::settings（P1.12；形状 1 判定＋一个组件）

```rust
pub struct AttachForm { pub name, pub base_url, pub dialect: Option<DialectKind>, pub secret: Option<String> }
pub enum AttachReadiness { Ready, NeedsName, NeedsUrl, UrlNotSafe, NeedsDialect }
pub fn ready(&AttachForm) -> AttachReadiness;           // 「完整表单」的唯一定义
pub fn url_is_safe(&str) -> bool;                       // https 任处；http 只到本机
pub fn attach_command(&AttachForm) -> Option<WireCommand>;   // 未就绪即 None，不造半成品
pub fn endpoint_rows(&EndpointsAnswer) -> Vec<EndpointRow>;
pub fn tag_rows(&EndpointsAnswer) -> Vec<TagRow>;       // 未选的标签也列，并说出代价
pub fn can_dispatch(&EndpointsAnswer) -> bool;
pub fn enrolment_note(&Enrolment) -> (Option<String>, String);
#[component] pub fn Settings(answer: Option<EndpointsAnswer>, on_frame: EventHandler<ClientFrame>) -> Element;
```

- **页面不判定任何事**：上面七个纯函数在宿主目标上全数可测，组件只读它们。「这次注册算不算完成」只有 `ready` 一个出处；`attach_command` 在未就绪时返回 `None`，而不是造一个半成的 Command——后者就是第二个定义。
- **凭证不进 Command，也不留在页上**：密钥字段输完即发往 `POST /enroll`（实现在 `web::socket`——模块表写明它是本 crate **唯一通信处**），回来的是 `secret:realm/name`；页面随即清空输入框。非浏览器目标上 `enrol` 直接答拒，而不是假装存了。
- **URL 安全判据在前端再守一次**：https 任处、http 只到本机。服务端同样守（`native::is_loopback`）；前端这一道不是第二个权威，是让人在**把密钥敲进去之前**就看到拒绝。
- **未选的标签也列出来且说出后果**（`consequence`）：只列已配置项的设置页，恰好藏起了人来这里要修的那一行。
- **P3.06 补上了两个表单**：`select_ready`／`select_command`（设置页，同 `ready`／`attach_command` 的形状）与 `city_view::create_command`（城市页）。两处都把服务端会给的拒绝提前到人按下去之前：端点没列的模型选不中，带斜杠的地址建不了楼。**不是第二个权威**：服务端同样拒，这一道只是早说一声。上下文窗与输出上限**不在表单里**（传 0）：它们是模型的事实，服务端持目录；一个人在表单里编出来的上限会在日后截断 Run，而那个理由不会出现在账上。
- **P1.12 未交付**（已由 P3.06 消掉）：`SelectModel` 还没有表单（今天只能由 `AttachEndpoint` 后走服务端或帧发出）；浏览器内往返仍未驱动。两件都记在 ARCHITECTURE §10 卡下。
- **产物读数**：278,103 B（gz，本卡后），预算 2 MB，余量 7.5 倍；前值 250,079 B（P1.05）。
- **框架口径**：Dioxus 0.7 默认提交表单，故 `onsubmit` 里显式 `prevent_default()`（官方迁移指南：<https://dioxuslabs.com/learn/0.7/migration/to_07/>）。

### 8-14 相机、选中面与反注意力验收（P4.13）

```rust
pub struct Camera { pub tile_width: u32, pub tile_height: u32, pub pan_x: i32, pub pan_y: i32 }
impl Camera { pub fn at_stop(self, stop: usize) -> Self; pub fn panned_by(self, dx: i32, dy: i32) -> Self; }
pub const PAN_STEP: i32 = 64;
pub fn dispatch_command(building: &str, task: &str, goal: &str) -> Option<ClientFrame>;

// socket：没人在看的时候，链路停，不是变慢
pub enum LinkState { /* … */ Suspended { resume_from: Option<Seq> } }
pub enum LinkEvent { /* … */ Backgrounded, Foregrounded }
pub enum LinkAction { /* … */ CloseSocket }
```

- **缩放与平移只改 `project`／`unproject` 这一对方法**：pan 在投影时加、反投影时减，tile 比例在每一档**重算**而不是缩放。于是「画得出来的就是点得中的」在每一档、每一个偏移下都成立，并由一条遍历断言钉住。
- **三档而非滑杆**：连续缩放会让人去找「合适的那一级」，而不是读这座城。越过最后一档即停，不回绕到最小视图。
- **选中之后能派活，且恒派到房间**：楼根上的 Run 会持有整栋楼的写域。task 与 goal 两者皆必填——目标是编的，Run 就报不出「做完了」。
- **反注意力验收落成五条断言**（`crates/web/tests/attention.rs`）：无未读计数（按词匹配，`unreadable_rows` 不误伤）、直播有窗口且报出丢了多少、进度条无动画、后台标签页**关闭链路**而非放慢、渲染模块必须也渲染文字。
- **后台即关，不是放慢**：放慢的标签页仍持有 socket、仍会唤醒机器、仍在花没人同意花的东西。回来时从梯子最底层重连——离开一段时间不是服务端有病的证据。

### 8-13 web::city_view 的绘制侧（P3.05；形状 1 判定＋Humble Object）

```rust
pub const CITY_EXTENT: u32 = 12;                       // 哈希落位的方格边长
pub fn prisms_of(buildings: &[BuildingProgress], busy: &BTreeSet<Address>) -> Vec<Prism>;
pub fn unreadable_rows(buildings: &[BuildingProgress]) -> Vec<String>;
#[cfg(wasm32)] pub fn paint(canvas: &HtmlCanvasElement, list: &DisplayList) -> Option<()>;
#[component] pub fn CityView(city: Option<CityAnswer>, busy: BTreeSet<Address>,
                             selected: Option<String>, on_frame: EventHandler<ClientFrame>,
                             on_select: EventHandler<Option<String>>) -> Element;
fn canvas_pixel(value: f64, bound: u32) -> i32;        // 私有；先限幅再转，故转换是全的
// web::theme
pub fn gray_colour(token: &str) -> Option<String>;     // 画布要的是值，不是 `var(--G7)`
```

- **`paint` 返回 `Option<()>` 而非 `Result`**：它能遇到的全部失败都是「浏览器没给我这个对象」，而那没有第二段可写——一个只能说「没画成」的 `AxError` 会向错误表里加一个没有 recovery 的码。
- **坐标转换只此一处，且先限幅**：浏览器给 `f64`，而 `i32` 没有 `TryFrom<f64>`。限幅到画布范围后转换是全的（Rust 的浮点→整数转换饱和），`NaN` 归零——左上角，什么都没点中。这是全库唯一一处 `as_conversions` 的 `#[expect]`，带理由带断言。

- **一幅图的两个数据源，刷新率不同**：楼的位置与高低来自 `CityAnswer`（一次查询，楼很少新建）；哪栋楼正在干活来自 `Snapshot` 的运行中 Run（事件流，逐条折入）。于是画面随活儿亮暗而不靠轮询——**把 fold 变成 poll 是把一个已解决的问题重新问一遍**。
- **高度取自路线图的分母**：一栋楼的“大小”是它揽下的活而不是它干完的活（`storeys` 取对数阶，故一栋巨楼不把城压成一片）；无计划的楼恒一层——没有分母就没有高度，而不是拿步数充数（同 `Progress` 两态的类型约束）。
- **画布只用灰阶**：`face_tokens` 返回的恒是 G 色标——形体由明度差立住，而色是冗余层（机械规则三）。故 `gray_colour` 只管灰阶一张表；一个只靠色相区分的面在去色后就不存在了。色值的两个生产点（CSS 自定义属性与画布字符串）读同一张 `GRAY_RAMP`，不是两个权威。
- **不可读的路线图行在页上看得见**：`problems` 不静默丢——一张惄悄少了两行的计划比没有计划更坏（同 `ledger_view` 的过滤计数口径）。
- **绘制侧是 Humble Object**：`paint` 零判定，只把 `DisplayList` 翻成 canvas 调用；几何全在宿主目标可测的纯函数里。点击路径同理：把坐标交给 `pick`，而 `pick` 与 `draw` 读同一个 `faces_of`。

### 8-15 装配：左栏、四个视图、控制面（R1.06；形状 7 router ＋ 形状 1 纯函数）

P4 收口时本 crate 的六个视图里只有两个有组件：`Root` 的 `nav` 是空元素，`View::Live/Approvals/Dashboard/Ledger` 四个臂渲染空 `div`，`view` 信号全 crate 无一处 `set`——于是浏览器里只到得了城市页，设置页在真实页面上不可达。库面测试全绿，因为它们测的是纯函数；**没有一条门在问「这个组件挂进树了没有」**。本节把缺口补齐并留下防复发的验收。

```rust
// web::app —— 路由与装配
pub struct Destination { pub view: View, pub label: &'static str, pub waiting: Option<u32> }
pub fn destinations(snapshot: &Snapshot) -> Vec<Destination>;      // 纯；左栏的唯一来源
pub const HELD_RECORDS: usize = 2_000;                             // 客户端保留的事件条数
pub fn spend_line(snapshot: &Snapshot) -> String;                  // 钱与 token 的唯一措辞
#[component] pub fn Root(snapshot: Snapshot, view: View, endpoints: Option<EndpointsAnswer>,
                        city: Option<CityAnswer>, cost: Option<CostAnswer>,
                        records: Vec<EventRecord>, selected: Option<String>,
                        following: bool, on_frame: EventHandler<ClientFrame>,
                        on_select: EventHandler<Option<String>>,
                        on_view: EventHandler<View>,
                        on_follow: EventHandler<bool>) -> Element;

#[component] pub fn LiveView(feed: Feed, run: Option<RunId>, following: bool,
                            on_frame: EventHandler<ClientFrame>,
                            on_follow: EventHandler<bool>) -> Element;              // web::live
#[component] pub fn ApprovalsView(items: Vec<ApprovalItem>,
                                 on_frame: EventHandler<ClientFrame>) -> Element;   // web::approval
#[component] pub fn CostsView(answer: Option<CostAnswer>, usage: Usage,
                             on_frame: EventHandler<ClientFrame>) -> Element;       // web::dashboard
#[component] pub fn LedgerView(records: Vec<EventRecord>,
                              on_frame: EventHandler<ClientFrame>) -> Element;      // web::ledger_view
```

- **左栏是 `destinations()` 的渲染，不是六个手写按钮**：目的地的措辞与「等你几件」的徽章各只有一个产地，于是新增一个视图改一处而不是三处。
- **组件与它的纯函数同住一个模块**（沿用 `city_view`／`settings` 已有形状）：`Root` 只路由不判定，四个视图各自把已有的 `inbox`／`cost_rows`／`page`／`describe` 渲染出来。**判定仍不在组件里**，这是 Humble Object 而非把逻辑搬进界面。
- **预算面撤出派活条**（用户裁定 2026-08-22）：一个人在派活之前说不出这件事值多少钱，而订阅计划连单价都不存在——一个填不出正确值的输入框只会教人乱填。`BudgetCap::default()` 随命令走，钱在事后如实报。
- **钱不确定时不装作确定**：`model_returned` 的 `billed_usd_micros` 缺席即「provider 没有报价」，页面报 token 与调用次数并说明为何无价，**不报 `$0.00`**——零和未知是两件事，而把未知渲染成零正是一份账目失去信用的方式。
- **审批面折自事件流的整项**：`ApprovalRequested` 的载荷就是 `ApprovalItem` 自身，故客户端反序列化整项、按既有 `inbox()` 聚类；wire 的 `ApprovalsAnswer` 同批改为携整项（见 `channels-SPEC.md` §8），于是「什么算一类」全库只有一处答案。
- **账本页只看得见本次连接以来的事件**，因为服务端只广播不回灌。页面把这句话写在过滤计数旁边；把历史回灌成查询是 R1.07 的事，**在它到来之前这一页不假装自己看得见全部历史**。

验收（防复发，`crates/web/src/app.rs` 测试模块）：把 `Root` 交给 `VirtualDom` 真渲一遍，遍历 mutations 收集元素标签与文本，断言

1. 左栏渲出 `destinations()` 的每一个目的地；
2. 六个 `View` 变体各自渲出该视图的实体标记（空 `div` 立即红）；
3. 派活条渲出 addr／task／goal／mode 四个输入且**不含预算输入**；
4. 右栏的钱一行在无报价时不出现 `$0.00`。

### 8-17 问答的时机与失效（R1.08）

```rust
pub fn invalidated_by(kind: EventKind) -> Option<Query>;   // 纯；哪条事件让哪个答案过期
#[component] fn Root(..., live: Signal<bool>, ...)         // 帧通了没有
```

- **页面在链路活了之后再问一次**：首次挂载早于握手完成，而未连通时发出的帧按设计**丢弃不排队**（队列就是「人要求了什么」的第二个住处）。故四个会提问的页监听 `live`，在有人可问时再问一遍——否则首屏永远停在「asking the city what it holds」。
- **事件只负责宣布过期，不负责折出答案**：`endpoint_attached` 之后重问 `EndpointView`，而不是在客户端自己拼一份 endpoint 表——后者是第二个权威。未修之前：接完 provider，模型下拉永远是空的，整座城因此派不出一次活。
- **带 prop 的 hook 必须 `use_reactive`**（Dioxus 官方：`use_effect` 只捕获首次渲染的 prop）。画布就是这样只画了一次：地面在、楼不在。

### 8-18 等距城市真的画出来（R1.09）

```rust
pub struct Camera { /* … */ pub viewport_width: u32, pub viewport_height: u32, pub extent: u32 }
impl Camera { pub fn origin(&self) -> (i32, i32); }         // 城在视口里居中
pub fn occupied_extent(prisms: &[Prism]) -> u32;            // 只担住人的那块地
pub fn ground_of(camera: &Camera) -> Vec<Face>;             // 底盘＋棋盘瓦
pub fn windows_of(camera: &Camera, prism: &Prism) -> Vec<Face>;
pub fn labels_of(camera: &Camera, prism: &Prism) -> Vec<Label>;
```

- **`fit` 之前不定原点等于没有 fit**：旧 `fit` 只算瓦块尺寸、把原点留在 `(0,0)`，于是所有 `v > u` 的瓦块落到负坐标——页面上是一块空画布加左上角一条灰色碎片。现在 `origin()` 把城放在视口中间，并留出塔高的头顶空间；一条断言遍历四个角与一座八层高楼，要求它们全在画布内。
- **镜头担当的是被占用的那块方格，不是哈希的全场**：两栋楼散在 12×12 里是两个小点；`prisms_of` 把坐标平移到包围盒角上（平移不破坏确定性，也不拆散绘制与命中的同一套几何）。
- **楼自己报名字与自己的数**（`labels_of`），不设图例；未点亮的窗也画（`windows_of`），因为「亮」只有在旁边有不亮的时候才是一个意思。

### 8-19 说清楚是哪座城、哪个 Run、历史的哪一页（R1.10）

```rust
pub fn watchable(snapshot: &Snapshot) -> Vec<(RunId, String)>;   // 新到旧，带相位的词
pub fn short_run(run: RunId) -> String;                          // 按钮上的短名
impl Snapshot { pub fn adopt_city(&mut self, city: Address); }   // 握手告知，不是折出来的
#[component] pub fn LiveView(..., runs: Vec<(RunId, String)>, on_watch: EventHandler<Option<RunId>>)
```

- **看哪个 Run 是选择，不是猜**：两个 Run 在跑时「最新那个」是抛硬币，而页面之前不告诉人它抛了。现在列出已知的 Run，并保留「全部」一档。
- **账本翻页按页数而不按 seq**：过滤器一改，同一个 seq 就不在同一页上了；存 seq 等于存一个会自己过期的指针。
- **城市名字写进创世记录**（`init` 的 `addr`），握手带回来；旧城回退到目录名。名字是一个事实，而事实归 Ledger——之前它只活在文件系统的目录项里，于是每个界面都在一座跑了一个月的城上写「no city」。
- **列表长了就收起来**：46 个模型 id 并排不是对「它服务 46 个模型」的阅读；计数在前，清单在一次展开之后。
- **画布固定像素尺寸、按窗格缩放**：一张图整体缩放，而不是让页面长出一条横向滚动条。

### 8-21 设置页的订阅登录（R1.14；形状 1 判定＋Humble Object）

`login_command(provider, step) -> Option<WireCommand>` 是纯的：两步各自铸自己的 `IdemKey`（材料含 step 与 code），于是「开始」按两次只开一次登录，而「完成」带着自己的键。授权 URL **不留在页面局部状态**，它由 `login_started` 事件进 `app::Snapshot`，`secret_captured` 到达即清空——「该开的 URL」和「登录已完成」都是服务端说的事实，页面只呈现。验收两条渲染断言：无待办时页面说「no login is waiting」而不是画一个空框；URL 到达后页面上出现的就是服务端记下的那一条。

### 8-20 楼页：一栋楼自己写下的东西（R1.11；形状 1 判定＋Humble Object）

```rust
pub enum Leaf { Doc(String), Archive }
pub fn opening_leaf(answer: &BuildingAnswer) -> Leaf;   // 有计划先看计划
pub fn progress_line(answer: &BuildingAnswer) -> String;// 计划自己的数，或者承认没有分母
pub fn day_label(day: u64) -> String;
#[component] pub fn BuildingView(addr: Address, answer: Option<BuildingAnswer>,
                                 live: Signal<bool>, on_frame: EventHandler<ClientFrame>);
// channels 侧：Query::BuildingView { addr } → Answer::Building(Box<BuildingAnswer>)
```

- **楼的记忆是文件，不是数据库**：`Roadmap.md` 是唯一任务表，`Memo.md` 收决定与更正，`Handoff.md` 是给下一位的，`BUILDING.md` 是规矩，`Archive/` 是归档。此前人要读它们只能离开界面去开编辑器。
- **只读，不编辑**：在这里改文件等于开出第二条改楼的路——没有 Run、没有账本行、没有检查点。人在这里能做的是读，然后派活。
- **进不来就没有页**：楼页不进左导航（一座城可能有五十栋），入口是城市页选中一栋楼后的那个按钮。
- **文档有上限并说明自己被截断**（`DOC_BYTES_MAX`）：这些文件会长几个月，而对截断保持沉默正是「视图」与「谎」的分界。

### 8-16 观感层：页面壳的样式表（R1.07）

组件挂上之后，页面壳里只有五区 grid 与四个颜色变量，于是每个表单、表格、列表都是浏览器默认样式。样式表因此扩到组件层，**仍在 `crates/web/assets/index.html` 一处**——它是 build.rs 嵌进二进制的那份，也就是浏览器真正拿到的那份。

- **恒不命名颜色**：每条规则读 `var(--G*)`／`var(--ACCENT*)`／`var(--ALERT*)`／`var(--PROGRESS_DONE)`，令牌由 `web::theme` 在首帧前写进 `:root`。`xtask color` 的仓库扫描因此对这份文件仍然成立。
- **意义由明度承担，ALERT 只说一件事**：需要人。它出现在待批徽章、被过滤掉的条数、无报价的说明、污染项的边框，别处不出现。
- **一套语法覆盖所有页**：按钮一个形状一个焦点环；表格一套表头与分隔线；`.empty`／`.window`／`.dropped` 这类「这一页为什么没东西」的句子统一为 G6 小字——**空态是内容，不是缺失**。
- **尺寸全部是 4px 的倍数**，数字用等宽数位（`tabular-nums`），于是钱与 token 在两行之间对得齐。
- 未做：暗／亮双主题（今天只有暗面）、密度切换、动效——进度条无动画是既有裁定，此处不引入第二个。

### 8-22 回收站有页（F1.01；形状 1 判定＋Humble Object）

```rust
pub struct BinRow { pub what: String, pub discarded_at: TimeMs,
                    pub return_path: ReturnPath, pub restored: bool }
pub fn bin_rows(answer: &DiscardAnswer) -> Vec<BinRow>;   // 线格式→视图的唯一翻译点
#[component] pub fn RecycleBinView(answer: Option<DiscardAnswer>, live: Signal<bool>,
                                   on_frame: EventHandler<ClientFrame>) -> Element;
```

- **`ReturnPath` 自 S4.07 就在，而到 R1 末无人调用**：服务端把 `Restoration` 拼成句子发过来，于是同一件事有了两个措辞处。本卡把计划本体放上线（channels-SPEC §8 的 F1.01 段），页面只读 `ReturnPath::sentence`。
- **`BinRow` 删掉 `bytes`**：`file_discarded` 的载荷里没有字节数，而一个恒为 0 的列不是「暂时没有数据」，是一个每行都在说谎的列。改携 `restored`，因为那是记录真的知道的事（`discard_restored` 翻它）。
- **已还原的行照显并标出**：一行回得来的记录是「返回路径真的能走」的证据；把它从清单里拿走等于只展示失败。序仍是最新在前（S4.07 定的那条：人要找的几乎总是刚刚那一次）。
- **页面不提供「还原」按钮**：线格式上没有 `Restore` 命令，而一个按下去没反应的按钮比没有按钮更坏。页面给的是可执行的句子（从哪个 checkpoint／哪个 CAS 地址拿）。

### 8-23 房间的信箱（F1.02；形状 1 判定＋Humble Object）

```rust
pub enum Leaf { Doc(String), Archive, Room(String) }   // 房间是楼的第三类面
pub enum RoomQueue { Unasked, Empty, Waiting(Vec<SignalLine>) }
pub fn room_addr(building: &Address, room: &str) -> Option<Address>;
pub fn waiting_in(inbox: Option<&InboxAnswer>, building: &Address, room: &str) -> RoomQueue;
impl Snapshot { pub fn signals_seen(&self) -> u64 }     // 计数，非队列
```

- **房间只在一处列出**：楼头原有的 `rooms: a, b, c` 文字行删除，改成与文档、归档同形的 tab。两张同义清单会让人问「哪一张是全的」，而那个问题没有好答案。
- **`Unasked` 与 `Empty` 分开**：另一个房间的答案、或尚未到达的答案，恒不得被画成「这里没有人在等」——那是一个本页没有依据的断言。判定写成纯函数，组件只 `match`。
- **看一眼不是取走**：页面恒不发 `pull`；队列由城从 Ledger 折出（channels-SPEC §8 的 R1.16 口径一）。页上那句话把这件事说出来，因为一个人看到队列时会想知道自己是不是刚刚拿走了它。
- **新鲜度靠计数而不靠轮询**：`Snapshot::signals_seen` 只数 `signal_enqueued`／`signal_consumed` 两类事件，变了就重问一次。它**恒不折队列内容**——那样就有了第二个「这个房间里有什么」的权威。`invalidated_by` 治不了这一条，因为它返回的 Query 不携地址；不为它另建一个机制，而是把「变过」交给那一页自己读。

### 8-24 归档页：搜索问盘，最近入库问账（F1.03；形状 1＋新模块 `web::archive_search`）

```rust
pub const FILED_LATELY_MAX: usize = 20;
pub fn searchable(needle: &str) -> Option<String>;      // None ＝不问，不是搜全部
pub struct Shelf { pub building: Address, pub hits: Vec<ArchiveHit> }
pub fn shelves(answer: &ArchiveAnswer) -> Vec<Shelf>;   // 按楼分组，确定序
pub fn filed_lately(answer: &RegistryAnswer, most: usize) -> Vec<RegistryLine>;
pub fn filed_line(answer: &RegistryAnswer, shown: usize) -> String;   // 把封顶说出口
```

- **一页两个源，页面自己说得出哪半边是哪个**：搜索走 `ArchiveSearch`（被问那一刻读盘，文件是权威）；「最近入库」走 `RegistryView`（从 Ledger 折 `asset_archived`，因此它能说出**什么时候**入的库、由哪个地址入的）。**两份恒不合成一列**：同一条可以两边都在，而一个分不清自己在读盘还是读历史的人，就分不清两者不一致时该信哪一个。
- **为什么不另开一个「登记」页**（F1.01 卡下已记）：两个查询答的是同一批东西。两个目的地、两张同义清单，是把「哪张是全的」这个无解的问题交给使用者。
- **空 needle 恒不发搜索**：空串命中全部条目，那等于在「最近入库」旁边再放一张完整清单，而且要为它走一遍全城的盘。按钮在无词时禁用，不是点了才拒。
- **封顶说出口**：`filed_line` 报「共 N 条，显示最近 M 条，还有 K 条更早」——静默截断与 `ledger_view` 的过滤计数是同一条口径。

### 8-25 体征与打扰（F1.04；形状 1＋新模块 `web::vitals`）

```rust
// web::vitals
pub struct Sign { pub what: &'static str, pub count: u64 }
pub fn signs(answer: &MetricsAnswer) -> [Sign; 3];      // 七个数只报三个
#[component] pub fn Vitals(answer: Option<MetricsAnswer>, live: Signal<bool>, on_frame);
// web::alert
pub fn alert_for(record: &EventRecord) -> Option<Alert>;
pub fn cleared_by(record: &EventRecord) -> Option<String>;
pub fn absorb(alerts: &mut Alerts, record: &EventRecord) -> Raise;
#[cfg(wasm32)] pub fn ask_to_interrupt();  #[cfg(wasm32)] pub fn interrupt(&Alert);
```

- **七个数只报三个，因为另四个已经有家**：`buildings`／`runs_active` 住城市图与楼索引，`runs_frozen` 住直播页，`approvals_waiting` 住左栏徒标。一个数两个家，总有一天会当着一个无法分辨真伪的人互相矛盾。剩下三个是别处真的说不出口的：账本长度（客户端只看得见连上之后，账本页自己就这么写着）、全城等着的信号（楼页一次只看一个房间）、未取回的删除（回收站列行不计数）。**拒绝列写在模块头的表里，并由一条断言看守。**
- **读数是一个时点**：开页即问，不保温。逐事件重问就是换个写法的轮询，而这三个数正是 fold 算不出来的那几个——那就是它们为什么是一个 Query。
- **`alert` 的生产消费者是浏览器通知，不是第二个屏上标记**：左栏已经说了「几件在等你」，顶栏再添一个点就是同一件事画两遍——那正是一个界面教会人忽略它的标记的方式。因此删掉了 `outstanding()` 与 `anyone_needed()`（两个无生产消费者的访问器，公开面只降不升）；`Alerts` 只剩 `raise`／`clear`，它的职责是**同一件事只打扰一次**——包括断线重连后事件被重送的那一次。
- **判定与事件同一遍读完**：`absorb` 紧挨 `Snapshot::apply` 调用。另开一个事件消费者就是第二个读流的人，而两个读流的人总有一个会落后。判定全在宿主目标可测（三条断言），wasm 侧只剩一句 `Notification::new_with_options`。
- **权限不阻塞**：`ask_to_interrupt` 发完就走；没授权就不通知，而不是弹框拦住城。通知携 `tag = alert.key`，于是浏览器自己也不会叠出第二份。

### 8-26 progress bar 的三个调用方（F1.05；形状 1 既有判定的接线）

```rust
pub enum Subject { Plan, Run }                      // 无分母时，两个主语知道的东西不同
pub fn bar(progress: &Progress, blocked: bool, subject: Subject) -> Bar;
#[component] pub fn ProgressBar(bar: Bar) -> Element;   // 全库唯一画它的地方
```

- **三处各自描述进度的代码合并成一处**。本卡之前：`city_view::note_of` 写「3/7」／「no plan」；`building_view::progress_line` 写「3 of 7 rows done」／「no readable plan…」；`progress::bar` 写第三套而无人调用。定义权威是「progress bar 只有一个渲染处」，故前两个函数**删除**，三个页面同读 `bar()`。
- **三个真实调用方**：城市页的楼标签（取 `label`）、楼页报头（整条 `ProgressBar`）、直播页的 Run 选择器（取 `label`，因此客户端一直在折却从未示人的 `steps_done` 终于有了去处）。
- **不接的那一处，及其理由**：`dashboard` 的条是**成本占比**，不是进度。共用一个形状会让同一根条在两页上意思不同，而那比两个形状更贵。
- **`Subject` 是参数而不是两个函数**：有分母时两者措辞必须一致（否则就是刚删掉的那种漂移）；无分母时才分岔，**因为两个主语知道的东西不同**：一个 Run 走过几步、花了多少钱都是事实，而一栋读不出 `Roadmap.md` 的楼什么数都没有——把它画成「0 steps」是在报告一个从未发生的 Run。
- **钱只在真有时出现**：线格式不携逐 Run 花费，故 `Subject::Run` 在 `usd == 0` 时只报步数。零与未知是两件事（同 `spend_line` 的口径）。
- **无分母即不画 fill**：宽度为零的 fill 在声称「什么都没完成」，而真实情况是「不知道完成了多少」。blocked 的竖纹因此在两种情形下各有落点（fill 右缘／轨道左缘）。

### 8-27 观感第三轮：分组的左栏与一张弧边档位表（F1.06；形状 6 数据面＋形状 1）

```rust
pub struct NavGroup { pub label: &'static str, pub places: Vec<Destination> }
pub fn destinations(snapshot: &Snapshot) -> Vec<NavGroup>;   // 分组仍只有一个产地
// web::theme
pub const CORNER_SCALES: [(&str, u16, u16); 4];   // 名，半径 px，超椭圆指数 n
pub fn superellipse_tenths(exponent: u16) -> u16; // CSS 的 K 是 n 的一半
pub fn continuity_order(exponent: u16) -> u16;    // 阶 ＝ n−1
```

- **左栏从八个并排变成三组**：`happening now`（城市ー直播ー待批）、`the record`（账本ー归档ー回收站ー成本）、`setup`。八个并排项被读成一张要搜的菜单，而不是一个可以去的地方。**恒不折叠**：折起来的一组是把页面藏起来，那是同一个缺陷戴上一个控件。分组仍住 `destinations()` 一处，故新增一个页面仍然改一处。
- **弧边是一张档位表，不是散在样式里的十二个半径**：样式表里原有的 `2px`／`3px`／`4px`／`9px` 全数删除，改读 `--corner-*`；与颜色同理——呈现常量的产地在 `web::theme`。
- **每一档说得出它取到第几阶连续**：正圆角的曲率在直边接处从 0 跳到 1/r（G1）；超椭圆让曲率长出来而不是跳上去，沿弧长为 `s^(n−2)`，故阶 ＝ n−1。panel 取 n=4（G3）、card／control 取 n=3（G2）、pill 回到 n=2（徽标必须读作一个圆）。
- **CSS 参数是指数的一半**：MDN 写明 `superellipse(K)` 把椭圆方程的指数换成 `2K`，故 `round`＝K=1、`squircle`＝K=2。把它读成 log2 或直接填 n，都会把面板画成几乎方的角；一条断言钉住这个换算。以**十分之一整数**存储并格式化，同颜色的千分整数口径（两次构建不因浮点而差）。
- **非 Baseline，故只作渐进增强**：`corner-shape` 自 Chromium 139 起可用（Edge 同源），Firefox／Safari 尚无；不支持时 `border-radius` 仍在，角落回到正圆，没有一处功能依赖它。
- **可搬的是规则不是代码**：论据与档位思路取自 RefRain 的 `corners.zig`（用户的另一个项目），而那边是 Zig＋Native SDK 的 canvas 路径构造器，搬过来的只能是「哪种表面需要到哪一阶」这一条。
- **产物读数**：461,921 B（gz），预算 2 MB，余量 4.5 倍；涨幅与理由入 `xtask/budgets.toml`。

## 8-13 拒绝到得了屏幕（P2.01）

**病灶**：`socket.rs` 收到 `ServerFrame::Refusal` 后产出 `LinkAction::Report(err)`，而 `app.rs` 把它与 `WaitMs`／`OpenSocket` 归入同一条「不动快照」的臂里——**一个人在设置页点 attach，失败时页面一个字都不说**。`alert.rs` 的 `AlertKind::Refused` 早就写着「拒绝在人干活的地方已经看得见」，那句话当时不成立。

```rust
// web::alert（形状 1 decision）
pub struct Refused { pub code: String, pub what: String, pub recovery: String }
pub fn refused(error: &AxError) -> Refused;
```

- **拒绝不是 `Alert`，故不进 `Alerts` 去重**。`Alert` 是一件持续的事实（有人在等批、一个 Run 冻住了），故「一件事只惊动一次」；而拒绝是**对一个动作的回答**。同一个错 URL 点两次，是两个问题要两个答案；把去重加在这里，第二次尝试就又回到了页面什么都不说。
- **位置在 `refused: Signal<Option<Refused>>` 而非 `Snapshot`**。`Snapshot` 是「从事件折向前的、客户端相信的东西」；一条拒绝不在历史里，也不应当在里面。
- **画在 top-bar 而非遮罩层**：它是答案，不是打断。不遮住任何东西，也不要求先关掉才能继续干活。ALERT 只上边框与错误码；整条条带染成暖色会盖过徒标，而徒标是唯一一个「必须有人动手」的标记。
- **只能由人关掉，不会自己淡出**：一个在被读到之前就消失的答案，等于没人回答。
- **`recovery` 为空时说出来**，而不是渲染成一段空白——空白被读成「没事」。

**本章测试**：渲染断言页上真的出现 `refusal`／`refusal-what`／`refusal-way` 三个类（这正是本卡之前整个 crate 里没有任何一处能画出拒绝的证据）；无拒绝时不画条带；`recovery` 为空时仍给出一句话。

## 8-14 View 与地址栏（F2.01）

**病灶**：`View` 只活在一个 signal 里。没有深链、没有浏览器后退、没有书签，**除首页外任何一页都拍不到照**——前端因此也无法做回归测试。

```rust
// web::route（形状 1 decision）
pub fn to_fragment(view: &View) -> String;         // 恒以 `#/` 开头
pub fn from_fragment(raw: &str) -> Option<View>;   // 认不出就是 None
#[cfg(target_arch = "wasm32")] pub fn current() -> Option<View>;
#[cfg(target_arch = "wasm32")] pub fn go(view: &View);
```

- **取 fragment，不取 path**。path 路由要求资产路由对认不出的路径回 `index.html`，而那一句拒绝是一条安全判定（`ClientAssets::lookup` 闭合于一张固定表并拒路径穿越）。fragment **根本不发给服务端**，故书签、历史、深链全都成立，且不动那道站在 URL 与本机磁盘之间的判定。
- **地址栏是唯一权威**。点击写 fragment，`hashchange` 监听器再移动 signal——点击与浏览器后退因此走同一条路，不可能对「人在哪一页」产生两种说法。监听器住 `use_hook`，只挂一次；挂两次就是同一次变更被应用两遍。
- **认不出的 fragment 答 `None`，不悄悄回首页**。一条落不了地的链接是调用方也许想说点什么的事实；悄悄换个地方落地，是在不承认的前提下教人不要相信自己的书签。
- **往返是穷尽的**：测试里那张 `View` 列表被一个无 catch-all 的 match 咬住，故新增一个 variant 不给它 fragment 就编译不过——而不是作为一个没人能链接到的页面发布出去。

**真机验收**：`http://127.0.0.1:<port>/#/settings` 直接打开设置页并截图成功；本卡之前该页无法被拍到。

### 8-28 等距城市改画成 SVG，并把数据放进天际线（F2.02；形状 1 判定 ＋ Humble Object）

```rust
pub const TILE_WIDTH: u32 = 64;   pub const MARGIN: i32 = 96;
pub struct Camera { pub tile_width: u32, pub tile_height: u32 }
impl Camera { pub const fn tiles() -> Self; pub fn project(&self, u: i32, v: i32) -> (i32, i32); }
pub struct Frame { pub x: i32, pub y: i32, pub width: u32, pub height: u32 }
impl Frame { pub fn attr(&self) -> String; }
pub fn view_box(list: &DisplayList, stop: usize, pan: (i32, i32)) -> Frame;
pub fn done_band_of(camera: &Camera, prism: &Prism) -> Vec<Face>;
pub fn points_attr(points: &[(i32, i32); 4]) -> String;
// 删除：paint / paint_mounted / canvas_pixel / pick / contains /
//       Camera::{fit, origin, at_stop, panned_by, unproject}
```

**推翻了 §3 那条记录**（理由已就地写入 §3）：那条的论据是「一千个 Resident 不做一千个节点」，而这幅图从未画过 Resident——它画 Building，一座城几十栋。为一个不存在的节约，canvas 正在支付四件确定的代价：固定位图被 CSS 重采样；读不到 CSS 自定义属性（`city_view.rs:703` 想要 `--ACCENT` 却只能退到 `G10`）；hover／focus／键盘三样都要自己重写；绘制只存在于 wasm，宿主侧的门与测试**一行都盖不到它**。

- **命中测试不再是第二次推导**：浏览器对它自己画出的 polygon 做命中，于是「画得出来的就是点得中的」从一条断言变成一条构造。反投影、凸四边形内判定、指针坐标限幅三者随之删除（含全库唯一一处 `as_conversions` 的 `#[expect]`）。
- **viewBox 取显示表自己的包围盒**，图因此填满给它的空间。旧 `fit` 横向为一个 n 瓦片宽的菱形预留 `2n+1` 个瓦片宽，超额约两倍——那就是三栋楼的城缩成中间一小块的来源。**包围盒不预留，它量。**
- **缩放只能是裁剪**：窗口跟着内容走时，瓦片放大与窗口放大互相抵消，画面纹丝不动。故瓦片成常量，三档缩放改为按档位收窄 viewBox，平移改为移动窗口中心；`Camera::fit`／`origin`／`at_stop`／`panned_by` 随之删除。一条断言钉住「档位越靠后窗口越小」，以免日后被「修」回瓦片缩放。
- **天际线就是数据**（`done_band_of`）：楼高是计划揽下的活，从地面亮上去的那一段是已完成的部分，于是进度从天际线上读而不是从旁边的数字读。**差一行没完的计划在城的另一头也不得看起来像完了**（`min(storeys-1)`）；无分母即**不画带**而不是画一条零高的带——后者在声称一个比值。
- **一栋楼一个 `<g>`**，带 `tabindex`、`role="button"`、`aria-pressed` 与 `<title>`；Enter 与空格都选中它。hover 与 focus 说 stroke 而不说 fill：面的 fill 是由几何按令牌写在行内的，**一条需要盖过行内样式的规则总会在某处输掉**。
- **标签在全部楼之后统一画**：写在各自的组里时，近处的楼会把远处的楼名盖掉（真机截图抳出）。名字仍在组的 `aria-label` 里，故不看像素的读者什么都没失去。
- **标签横向留白是估算的**（`MARGIN`，§14 硬编码声明）：文本宽度要字体度量才知道，而宿主侧没有；`text-anchor: middle` 使估小了也只是两端各差几个单位。
- **败诉线**：这幅图若有朝一日要画 Resident，或城中楼数越过 200（实测一栋楼约 30 个节点），节点数论据重新成立，届时回到画布并把本节改回去。

### 8-29 呈现常量收口，与两条覆盖全部页面的语法（F2.03；形状 6 数据面 ＋ 新模块 `web::panel`）

```rust
// web::theme —— 呈现常量的产地，从颜色与圆角扩到字与间距
pub const FONT_SANS: &str = "sans-serif";      // 读者在浏览器里选的那一个
pub const FONT_MONO: &str = "monospace";       // 同上，等宽那一项
pub const TYPE_SCALE: [(&str, u16, u16); 6];   // 名，px，字重
pub const SPACE_SCALE: [(&str, u16); 6];       // 名，px，恒为 4 的倍数

// web::panel —— 中栏的唯一版面语法
#[component] pub fn Panel(title: String, scope: Option<String>, figure: Option<String>,
                          source: String, children: Element) -> Element;
#[component] pub fn Empty(status: String, what: String, children: Element) -> Element;
```

**病灶**：组件挂上之后（§8-16）样式表只解决了「有没有样式」，没解决「谁比谁重要」。实测：全页字号在 11px 到 28px 之间有十一档，其中 12／12.5／13 三档彼此相差不到一像素而无任何规则说明新的一行该取哪档；`.centre` 是一个大方框，里面全部元素同处一个平面；十五处 `.empty` 各写一句话，读者分不清「还没有」「加载中」「被过滤掉了」。

- **字与间距进 `web::theme`，理由与颜色、圆角同条**：呈现常量只有一个产地。字阶六档、间距六档，**档名说的是用途不是尺寸**（`figure`／`title`／`heading`／`body`／`small`／`micro`），于是一档可以被重新调音而不会让它的每一个读者当场变错。一条断言回读 `assets/index.html`：样式表里再出现一个 `font-family` 或一个字号字面量即红。
- **不发字体，也不取字体，更不点名字体**（用户裁定 2026-08-24）。只写通用族 `sans-serif` 与 `monospace`——那正是浏览器「自定义字体」面板里的两项，于是**字体是读者的选择而不是我们的**。三条理由按决定次序：屏幕上只有 chrome 是我们的，其余是人写的内容（楼名、`Memo.md`、账本载荷），字符集不可预测因而子集覆盖不了，而整份 CJK 字族是几 MB 对两 MB 的预算；写 `system-ui` 或任何具名字族都会把选择权收回来；**旧样式表把 `Noto Sans SC` 排在 `system-ui` 前面，于是英文 chrome 一直由一个中文字族的拉丁字形绘制**——这是本卡红转绿抳出的真缺陷。设置页的 Interface 一节说出这个设置住在哪里：一个产品遵守却从不提及的偏好，是读者找不到的偏好。
- **暗色界面的深度靠明度阶梯，不靠投影**——投影不可能比近黑更暗。四级：G0 页面 → G1 工作面 → G2 卡片 → G3 抬起，交界处补一道 1px 的 G3 边。此前只用了两级，于是每块面板糊在一起。
- **`Panel` 四段是中栏的唯一版面**：标题（**结论，不是名词**）／副题（数的范围与图例）／主体／**数字从哪来**。第四段是这个产品不能省的那一段：一个把数字摆出来却说不出出处的面板，与「整座城的历史在一条可验证的 Ledger 里」这句话直接矛盾。一页是若干 `Panel` 的堆叠，别无其他，于是两页不可能对「一个标题是什么意思」产生两种说法。
- **`Empty` 三段**（取自 Nielsen Norman 的空态规则）：说清系统状态／说清这里本该有什么／给出把它填上的那条路。**恒不允许纯空**，也恒不允许把「还没有」写成与「加载中」同一句话——那正是 `RoomQueue { Unasked, Empty, Waiting }` 早已在一处做对、而其余十四处没有做的区分。

### 8-30 首屏：人抵达时带着的那个问题（F2.04；形状 1 判定 ＋ 新模块 `web::overview`）

```rust
pub struct Working { pub runs: usize, pub buildings: usize, pub raised: usize }
pub fn working(snapshot: &Snapshot, city: Option<&CityAnswer>) -> Working;
pub fn headline(working: &Working) -> String;          // 首屏存在的理由，一句话
pub struct Attention { pub what: String, pub count: u32, pub view: View }
pub fn needs_you(snapshot: &Snapshot) -> Vec<Attention>;
#[component] pub fn OverviewView(…) -> Element;
pub enum View { Overview, … }                           // 新的 `#[default]`，片段为 `#/`
```

**为什么城市图不该是首页**：它答的是「东西都在哪里」，而一个人打开这个产品时问的是「**有事在发生吗，其中有需要我的吗**」。两个不同的问题，只有后一个是抵达时问的。城市图保留 `#/city`，旧链接仍然落得下。

- **两个数，不能再多**：报七个数的首屏一个都没报——眼睛只会落在最大的那个上。故标题是一句带两个数的话，其余全是人可以走进去的清单。
- **句子按情形分峐，不是模板填数**：「0 runs in 0 buildings」与「这座城还没有楼」是同一个事实，而只有后者是一句话。三条断言钉住三种形状（空城／有楼无活／有活）。
- **数的是地方不是路径**：一栋楼两个房间各一个 Run，算**一栋楼在干活**。读这行的人在数地方。
- **只数飞行中的**：冻结与 halted 的 Run 列在下方但不计入首行——**一座停下来的城绝不得读起来像忙着**，这是首屏唯一不可犯的错。
- **「等你」的排序是变贵的顺序**：待批停的是当下一个 Run；冻结的已经停了且会一直停；provider 断了则停住一切尚未开始的。按类别排会把最便宜的那一行放到前面。
- **整行即按钮**：一句话里埋一个链接是让人去瞄准；被瞄准的就是那一整行。
- **不新增任何查询**：全靠事件流的 fold 加上别的页本就要问的一条 `CityView`。一个会轮询的总览页会是全库最贵的一页，而它能告诉人的东西 fold 都已经知道。

### 8-31 人对一个 Run 能做的事，不只是说话和停（F2.05）

```rust
pub fn takeover_command(run: RunId) -> ClientFrame;              // web::live
pub fn fork_command(run: RunId, at_seq: Seq) -> ClientFrame;     // web::live
pub fn rollback_command(checkpoint: &str) -> Option<ClientFrame>;// web::approval
impl GitOid { pub fn parse(raw: &str) -> Option<Self> }          // kernel::locator
```

**盘点的结果**：线格式上二十条 Command，界面发得出十一条。`channels::control` 把五条归为 Intervention（Steer／Cancel／Takeover／Rollback／Halt＋Release），而其中 **Takeover 与 Rollback 根本发不出去**。一个只有「说一句」与「停下来」两个动词的委派工作界面，不是 control surface，是一份带开关的记录。

- **Fork 的点只能是本页看得见的那一步**：`at_seq` 取直播窗口里最后一条的 seq——一个看着它跑的人能“意指”的就只有这一点。按钮写它**造出什么**（一条分支）而不是它启动什么：词汇表写明 Fork 只记谱系，不自己开始开。
- **Rollback 不是「还原这个文件」**，按钮就这么写：它把整个 worktree 拉回一个 checkpoint。回收站里返回路径是内容地址或重建说明的行**恒不给按钮**——线格式上没有那条命令，而一个按下去没反应的按钮比没有按钮更坏（延用 §8-22）。
- **`GitOid::parse` 开在 kernel 而不是在客户端重写十六进制解码**：那份文件的 `Deserialize` 旁边就写着「形状权威留在这里，以免 wire 长出第二个定义」——客户端同理。长度不对即拒，恒不补零猜。
- **尚未接出的三条，各自的阻塞写在 TODO**：`SetAutonomy`（需先定下 Owner／Deferred 在界面上各自意味着什么）、`BatchByBuilding`（`ApprovalItem` 不携 Address，从 `actor` 反推楼名是猜）、`Attach`（客户端根本没有上传面）。**写出来而不是假装它们不存在。**

### 8-32 三个页面在替一座它们刚认识的城作答（F2.06；真机仿真抳出）

```rust
pub struct Working { pub runs, pub buildings, pub raised, pub frozen, pub known }
#[cfg(wasm32)] pub fn route::unresolved() -> Option<String>;   // 地址栏说了什么而本构建认不得
// route：View::Dashboard 的片段改为 `#/cost`，`#/dashboard` 仍可解析
```

**仿真现场**：一座真实跑过四次 Run 的城。总览页写「no run has started in this city」，直播页写「no run has been dispatched in this city」，右栏写「nothing spent yet」——**三句话全是对整座城的断言，而三个来源都只是一个从页面连上才开始的 fold**。账本页早就用「本页只看得见连上之后」避开了这个坑；另外三处没有。

- **城的数在前，fold 只能抬高它**：`CityView` 答的是整部历史，fold 比上一次答案新，故**两者取大者**。不是两个权威：一个答「曾经有过什么」，一个答「刚刚又多了什么」。
- **冻结自己一句话**：「nothing is running」与「4 个 Run 停在了半途」不是同一件事，把后者渲染成前者等于把一座卡住的城画成一座闲着的城。
- **零与未知，在成本页上第二次被抳出**：ModelScope 按订阅计费，权威总额恒为 0，而归因里四个 Run 都在。页面原本把它们渲染成五列 `$0.00`。现在：标题说「做了活，没人报价」，每行写 `unpriced`，并不给 figure。
- **导航说 cost，地址栏却写 `#/dashboard`**：人照着看见的字敲进去，落到一个本构建解析不了的片段，然后**默默落在首页**——这正是 §8-14 明拒的行为。片段改成 `#/cost`（旧拼法仍可解析，因为别人存下的链接是一个本构建来不及收回的承诺），而认不出的片段**抬一条拒绝**（`E_NO_SUCH_PAGE`）而不是默默换个地方落地。

**本卡的真机证据**（ModelScope，Qwen3-235B）：模型先用 shell 臂调 exec → 读到 `E_TOOL_UNAVAILABLE` 及其 recovery → 改用 program 臂 → 读到 `E_INVALID_ARGS` 及其 recovery → 第三次写对并成功。**三段式拒绝对模型也是有效的，不只对人。** `pwd` 的输出落在 Run 自己的房间里，写域成立；429 变成 `provider_degraded` 携 recovery；冻结时写了带三个 must-read locator 的 Handoff。

### 8-33 一座新城与它的第一次派活之间的四道坎（F2.07；形状 1 判定 ＋ 既有组件重排）

```rust
pub(crate) fn models_of(answer: &EndpointsAnswer, endpoint: &str) -> Vec<String>;  // web::settings
#[component] fn DispatchBar(addr, on_frame, on_view)                               // web::app
```

**真机取证**（用户自己跑的那座城，`city/.sprawling/ledger/`，四条记录）：`city_initialized` → `secret_captured`（`secret:1/key`）→ `building_created` ×2。**没有 `endpoint_attached`，也没有一次 `Dispatch`。** 一个人把密钥放进了保险库、盖了两栋楼，然后停在了那里；这四道坎每一道都在他停下的那条路上。

- **页序即人能执行的次序**。设置页原本的顺序是：已接端点表 → 各标签派什么用 → 选一个模型 → 订阅登录 → **附一个 provider**。前三节在没有 provider 时全是空的，而唯一能让它们不空的那一节在第五位、在折线以下——连它自己的提交按钮都在窗口之外。改成：附 provider → 订阅登录 → 选模型 → 标签表 → 现在接着什么。同样的内容，倒过来的顺序，**本节不新增任何判定**。代价记明：一座已配好的城，回访者要多滚一屏才看见「现在接着什么」；面板抬头已经先答了「这座城派不派得出活」，故这一屏不是他要找的答案。
- **模型下拉按选中的 provider 过滤**。原本 A 家的下拉里列着 B 家的模型，于是 `SelectReadiness::ModelNotServed` 纯靠界面误导就能达成。过滤**不是第二个权威**（延用 §8-12）：服务端同样拒，这一道只是把拒绝提前到人点得到之前。换 provider 即清空已选模型——留着上一家的名字，就是留着一个必然被拒的表单。
- **派活条的四个控件带标签**。本仓库自己的样式表写着「A label above its field, never a placeholder standing in for one: a placeholder disappears the moment somebody types」，而派活条是全库唯一违反它的表单：四个控件全靠占位符说话，地址列 180px，在 1600 宽的窗口里把 `which room, as building/room` 截成 `which room, as building/ro`。地址列改宽并给出标签。
- **送出即落到直播页**。按下 `send it` 之后页面不动，一个人无从知道那一帧出去了没有；`on_view` 把视图切到 `View::Live(None)`。**不替人选 Run**——§8-31「哪个 session 是选的，不是猜的」仍然有效——直播页仍要人点一个 Run 才说得进话，只是没选时说的是「先点上面一个 session」，而不是给一个永远按不动的输入框。
- **城市页的画布封顶到 52vh**。§8-18 记的理由是「height: auto，让盒子取画本身的比例，不留空的信箱边」，那条理由在 1280×800 下的代价是：`raise a building` 表单与楼列表全部落到折线以下，最后一行被裁掉一半。**记录随现实更正**：比例仍由画自己定，只是高度封顶，短窗口下让出侧边的空白，好过让人滚一屏才找得到唯一能盖楼的地方。

验收：`Painted` 渲 `View::Settings`，断言 `Attach a provider` 的文本序号小于 `choose a model for a job`；渲 `View::Live(None)`，断言页面说出要先选一个 session 且**不含**可提交的 steer 表单；`models_of` 只答选中 provider 的模型；派活条渲出四个 `label`。

### 8-34 派活时给这次会话取个名字（F2.11 的客户端半边）

```rust
pub fn dispatch_command(addr, task, goal, mode, session: &str) -> Option<ClientFrame>;
fn city_view::session_name(task: &str) -> String;   // 任务的前四个词
```

- **派活条多一格「call it」**：填了就开一个新房间（地址栏给楼即可），空着就是向地址栏那个房间继续干。一个字段表达两种意图，而不是两个模式开关。
- **城市页不再把每一件活都扔进 `room1`**：原先 `city_view::dispatch_command` 写死 `{building}/room1`，于是同一栋楼上发出的第二件活盖掉第一件的文件。现在地址给楼，名字取自任务的前四个词——人刚写完的词，一小时后在文件夹列表里认得出来。
- **名字不合法就拒整条命令**（`SessionName::parse` 答 `None`），而不是自作主张改拼写：一个没人敲过的名字不应该出现在别人的目录里。

### 8-38 思考强度在按钮旁边（F2.16 的客户端半边）

```rust
pub fn effort_named(value: &str) -> Option<Effort>;   // 空值＝不选，不等于 Effort::None
const EFFORTS: [(&str, Msg); 6];                      // 六档，含「跟随全城设定」
```

- **空值与 `Effort::None` 是两件事**：前者是「这一层不表态，让上一层答」，后者是「明确要求不要推理」。一个下拉里同时存在这两种意思，必须分得开。
- **控件在 send it 旁边而不在设置页**（用户裁定）：人在派活那一刻正好在决定这件事，而一次会话只选一次。
- **城市页那个快捷表单不给这个控件**：它只问两行字和一栋楼，档位由整张表单所在的地方决定。

### 8-39 每一页迁入 web::lang（F2.18）

十个页面的可见文字全部改由 `Msg` 给出：总览、直播、城市、楼、审批、回收站、成本、账本、归档、设置，加上右栏体征。

- **纯函数返回 `Msg` 而不是句子**：`headline`、`describe`、`Attention`、`Sign`、`EndpointRow`、两个 `Readiness::sentence` 都改成交出消息与槽位，由组件在绘制处说出来。判定仍在纯函数里，只是不再兼任翻译。
- **带数字的句子配一个 `*_in(lang, …)` 字面**（`headline_in`、`describe_in`），调用方拿不到消息而忘了填值。
- **rsx 内插不能嵌大括号**，所以带槽的句子先落到一个 `let` 或一个小函数（`run_id_line`、`cut_empty`、`model_count`…）再进模板。
- **未迁入的尾巴（约 15 条，记在 TODO P0）**：`alert` 的通知文案、`app::status_line`、`progress` 的「no plan」、`route` 的拒绝句。它们都在不接语言参数的纯函数里，改签名是下一张卡。

### 8-37 web::lang：界面说谁的话（F2.14；形状 6 数据面）

```rust
pub enum Lang { En, Zh }                 // 两种，不是一张 locale 表
pub enum Msg { … }                       // 穷举；新增一条不翻就编译不过
pub struct Phrase { pub en, pub zh }     // 一条消息两种语言并排
pub fn phrase(Msg) -> Phrase;  pub fn say(Lang, Msg) -> &'static str;
pub fn preferred() -> Lang;    pub fn remember(Lang);
```

- **漏译不可表示**：一条消息就是一个 `Phrase`，两个字段必须都填。换成「每种语言一张表」就会多出一个能忘的地方；单测另外拒绝「中文栏里没有一个汉字」的假翻译。
- **语言走 context 而不走 prop**：人读什么语言是整页的事实，不是某一块面板的。`App` 提供，测试的 harness 同样提供——不提供就 panic，而不是静默退到英文。
- **默认取浏览器自己的设置**，选过一次就记在 localStorage；存不了不算错，选择在本页仍然生效。
- **译名跟 `README.zh-CN.md`**：城／楼／房间／会话／Ledger。一个概念两种叫法就是两个概念。
- **本卡的范围是人最先碰到的那一层**：左栏十二条、派活条十一条、停城与取消、语言开关自身。剩下的二百多条（panel 的 scope／source 长句为主）随后续卡分批迁入，**迁一条少一条硬编码**。

### 8-36 从一栋楼到它里面的活（F2.13）

```rust
pub fn room_asked_for(frame: &ClientFrame) -> Option<String>;      // building/name
pub fn started_here(record: &EventRecord, expecting: &str) -> Option<RunId>;
#[component] fn BuildingView(..., on_select: EventHandler<Option<String>>)
```

- **楼页不多一个派活表单**，而是把底栏指向这栋楼：开工只有一个地方，下一次人也往那里找。底栏的地址格因此需要 `use_reactive`（§8-17 那个坑），否则它永远停在页面打开那一刻的选中项。
- **送出后自动打开刚开的那个 session**：客户端记住自己要的房间（`room_asked_for`），在 `run_started` 到达时认出它（`started_here`）。**这不违反 §8-31**：那条裁定禁的是在几个 Run 之间猜，而本客户端发出了那一帧、知道它要的是哪个房间——知道不是猜。
- **后缀也算**：城可能把 `lab/refactor` 开成 `lab/refactor-2`，所以匹配允许 `-数字` 后缀；`lab/refactoring` 不算。
- **只认 `run_started`**：同一房间里的后续事件不得再把页面拽回去——人可能已经走开了。

### 8-35 选一个 session 而不是选一个哈希（F2.12）

```rust
fn app::session_of(id: &RunId, row: &RunRow) -> String;   // 房间名，否则短哈希
fn live::named(runs: &[(RunId, String)], run: RunId) -> String;
// watchable 的标签从「{phase} · {bar}」变为「{session} · {phase} · {bar}」
```

- **名字取自地址的最后一段**，也就是房间名，也就是人在「call it」里敲的那个词（F2.11）。客户端不另存一份名字表：`RunRow.addr` 已经是那个事实，再存一份就是第二个权威。
- **没有地址时回落短哈希**：一个本页没见过地址的 Run 仍然要有一个按得下去的按钮，难读好过空白。
- **Run 标识符不从页面上拿掉**，只是变安静：人读名字，而 `sprawling fork` 与账本寻址用的是它。
- **标题与按钮同源**：`named()` 从 picker 的同一份列表里取字，于是两处不会对不上。

## 8.5 两个设计（crate 级）——S4.01 前端框架结论书

> **地位**：本节即卡 S4.01 的产出。当时的要求是「结论书写明度量方法与败诉线，并记录被否方案的理由」；ARCHITECTURE §11 要求被否方案就地留痕于 SPEC 的「两个设计」节，不另设记录文件。
> **现行框架**：Dioxus 0.7.x（铉版见附录 A）。下文判据与败诉线是它的继续有效条件；触线即换，不重议判据。
> 「最新版本」我取**最新稳定版 0.7.10**，不取 `0.8.0-alpha.1`：alpha 的公开面按定义不稳定，而本仓库 `Cargo.lock` 入库、`rust-toolchain.toml` 钉版、cargo-deny 恒跑，钉一个 alpha 与这套纪律相悖。此读法若与你的本意不符，说一声即改。

### 8.5-1 判据与其排序（不可自行改动）

选型要求原文：「**前端框架落定**：Rust 编译到 WebAssembly 的方案，构建链不得引入 npm/node（C1）。选型判据按重要性排序：可持续维护的证据（有商业支持或生产部署，不是单人轻维护项目）；构建工具链和性能表现是否适合本项目。」ARCHITECTURE §4 的转述补一条末位判据：可持续维护的证据 ＞ 纯 Rust 构建链 ＞ 细粒度更新。

- **C1 是硬约束，不是判据**：构建链引入 npm/node 即出局，不参与加权。
- **判据一（可持续维护的证据）拥有否决权**：「单人轻维护项目」是写明的反面样本，命中即出局。
- **判据二（构建工具链与性能表现）在判据一的幸存者之间排序。**
- **判据三（细粒度更新）只在前两条打平时起作用**，不设否决权。

### 8.5-2 度量方法（可复算，取证日 2026-08-21）

| 量 | 定义 | 取值方式 |
|---|---|---|
| M1 发布新鲜度 | 取证日 − 最近一个**稳定版**（非 beta/alpha）发布日，单位天 | crates.io 版本历史 |
| M2 贡献集中度 | 第一贡献者提交数 ÷ 第二贡献者提交数；比值越高越接近单人项目 | GitHub 贡献者榜 |
| M3 组织支持 | 是否存在以该项目为业务的实体（雇员数、融资）或具名生产部署 | 公司主页／投资数据库／项目自述 |
| M4 维护者自述 | 维护者对未来维护强度的公开表述；自述优先于任何外部推断 | 项目 issue／公告 |
| M5 构建链外部件 | 从 `cargo build` 到静态资源，需要几个非 cargo 可执行件；其中几个能被 `rust-toolchain.toml`／`Cargo.lock` 钉住 | 官方安装与构建文档 |
| M6 npm/node 接触面 | 构建链是否调用或下载 node 族工件（`xtask zerojs` 的判据面） | 该工具的 changelog 与配置面 |
| M7 破坏性节奏 | 近 12 个月内的 semver 破坏性发布次数 | 版本历史＋迁移指南 |

M1 与 M2 是**证据**不是**结论**：一个功能完备的库可以合法地长期不发版。故 M4（维护者自述）在冲突时压过 M1/M2——这是判据一「证据」二字的含义。

### 8.5-3 候选与实测（五项，含「不用框架」）

| 候选 | 最近稳定版 | M1 天 | M2 | M3 组织支持 | M4 自述 |
|---|---|---|---|---|---|
| **Dioxus** | v0.7.10（2026-07-30） | 22 | 未取到分项（445 贡献者／7087 提交） | Dioxus Labs：YC S23、种子轮、全职团队；自述具名生产用户 | 活跃；0.8.0-alpha 在途 |
| **Yew** | 0.23.0（2026-03-10） | 164 | 未取到分项 | 无实体；社区驱动 | 活跃；2025-05 一名维护者公开退出 |
| **Leptos** | 0.8.20（2026-06-25） | 57 | 3387 ÷ 287 ≈ **11.8** | 无实体 | **2026-05 宣布减速维护** |
| **Sycamore** | 0.9.2（2025-09-23） | **332** | 520 ÷ 9 ≈ **57.8** | 无实体 | 无近期表述 |
| **不用框架**（`wasm-bindgen`＋`web-sys` 直用） | wasm-bindgen 0.2.127（2026-08-08） | 13 | —— | rustwasm 组织 2025-07 落幕后仓库转入新 `wasm-bindgen` 组织并增补维护者；4951 个下游 crate；有成文 MSRV 政策 | 活跃 |

**M4 的决定性证据**（Leptos issue #4707「Status Update - May 2026」，维护者第一人称原文）：

> "Leptos is not abandoned but will be lightly maintained going forward. I consider it feature-complete and do not expect to do significant new development in the future. I am open to additional maintainers who want to take a more active role."

取证时该 issue 未显示有人接手。这段话与反面样本「单人轻维护项目」逐字对应，加上 M2＝11.8 的贡献集中度，Leptos 在判据一上出局——**不是因为它不好，而是因为判据一问的是「谁会在两年后修它」**。

**M5／M6 构建链实测**：

| 候选 | 构建路径 | 非 cargo 外部件 | 可钉住 | npm/node 接触面 |
|---|---|---|---|---|
| Dioxus（走 `dx`） | `dx build --target wasm32-unknown-unknown` | 1（`dx`） | **否**：`dx` 自带并自动获取它自己的 `wasm-bindgen-cli`，覆盖 PATH 上的版本（DioxusLabs/dioxus#3457，取证时仍开放） | 无 |
| Dioxus（绕开 `dx`） | `cargo build` ＋ `wasm-bindgen` CLI | 1（`wasm-bindgen-cli`） | **是** | 无 |
| Yew／Leptos／Sycamore | `trunk build` | 1（`trunk`） | 是（版本可钉） | **有**：0.22.0-beta 的 changelog 含「add node-package configuration」与「download node package in crate folder」 |
| 不用框架 | `cargo build` ＋ `wasm-bindgen` CLI | 1（`wasm-bindgen-cli`） | **是** | 无 |

Trunk 的 npm 接触面是**可关闭的可选配置**，不构成 C1 的当场违反；但它是该工具的行进方向，意味着 `xtask zerojs` 要长期为它作证。同时 Trunk 的稳定版停在 0.21.14（2025-05-08，M1＝470 天），0.22 自 2026-03 起停在 beta——**判据二上，Trunk 这条路径比它服务的三个框架本身更脆**。

**M7 破坏性节奏**：四个框架**全部处于 1.0 之前**，且近 12 个月内各有一次 semver 破坏性发布（Dioxus 0.6→0.7、Yew 0.22→0.23、Leptos 0.8→0.9-beta、Sycamore 0.8→0.9）。这条对全部框架候选一致成立，故它不区分候选，但它定下了后文败诉线 L3 的必要性。

### 8.5-4 结论

**取 Dioxus 0.7.x，且构建绕开 `dx`：`cargo build --target wasm32-unknown-unknown` ＋ 钉版 `wasm-bindgen-cli`。**

四条理由，按判据序：

1. **判据一**：它是唯一同时具备两种点名证据的候选——商业支持（以该项目为业务的实体、融资、全职团队）与具名生产部署。Leptos 与 Sycamore 被判据一否决；Yew 只有社区一条腿，且 M1＝164 天。
2. **判据二（构建链）**：绕开 `dx` 后，构建链上的非 cargo 外部件只剩 `wasm-bindgen-cli` 一件，而它是**全部候选共同的地基**——选任何框架都躲不开它。于是 Dioxus 在构建链上的增量成本是零。走 `dx` 则不可接受：一个自动获取自己工具版本、覆盖 PATH 的 CLI，与本仓库「钉版工具链由 `rust-toolchain.toml` 自动安装」的纪律正面冲突，也与确定性构建冲突。
3. **判据二（性能）**：九个 DOM 模块的负载最重处是 `ledger_view` 的可过滤历史列表；signals 式细粒度更新在此有实效。`city_view` 走画布，与框架无关，故框架的渲染开销不进入 §18.5 的 3ms 帧预算。
4. **判据三**：Dioxus 0.7 的 signals 属细粒度一侧，优于 Yew 的虚拟 DOM。此条不承担决定性重量。

**随结论生效的三条硬规则**（写进本 SPEC 即成为实现约束）：

- **不启用 `asset` feature**。依赖行恒为 `dioxus = { version = "0.7.10", default-features = false, features = ["minimal", "web"] }`。`minimal` ＝ `macro, html, signals, hooks, launch`，**不含 `asset`，也不含 `devtools`**。官方 Agent Guide 写明「Assets use link sections and binary patching — the `asset!()` macro creates symbols the CLI processes」：没有 `dx` 它本来就不工作。关掉该 feature 使 `asset!` 宏**根本不存在**——禁令因此是机制而非纪律，与本库「让非法状态不可表示」同规。我们的资源通路是 `crates/web/assets/` → `build.rs` → `include_bytes!`。
- **`dx` 不得出现在 `justfile`、CI 步骤或 `build.rs`**：`just build-web` 只许是 `cargo build` ＋ `wasm-bindgen`。
- **`wasm-bindgen-cli` 与 `wasm-bindgen` crate 版本必须一致**，且写进环境前置文档（AGENTS.md 环境前置节）；版本不一致是 wasm 构建最常见的静默失败面。

**钉版实测**（2026-08-21）：`dioxus` 0.7.10，MSRV 1.83.0（本库 1.97.1 满足），许可 MIT OR Apache-2.0（B.7 相容）。`0.8.0-alpha.1` 是 crates.io 上的最新发布物但属预发布，不取。

### 8.5-5 败诉线（触发即启动替换，不上会重议判据）

| 线 | 条件 | 处置 |
|---|---|---|
| L1 | 上游连续 6 个月无稳定版发布，且未见维护者对此作出解释 | 启动替换评估 |
| L2 | 上游宣布减速维护，或全职团队解散，且 90 天内无接手实体 | 直接替换（这正是 Leptos 本次的形状） |
| L3 | 跨一个 minor 版本升级导致 `crates/web` 改动行数 >20% | 记一次；累计两次即替换（pre-1.0 的破坏性成本超预算） |
| L4 | 构建链出现无法被 `Cargo.lock` 或 `rust-toolchain.toml` 钉住、且无法用环境变量关闭的自动下载物 | 直接替换（C1 与确定性构建的共同底线） |
| L5 | 前端产物压缩后传输量 >2MB，且瘦身后仍超 | 直接替换 |

**替换目标恒为「不用框架」一档**（`wasm-bindgen`＋`web-sys` 直用）：它在判据一与判据二上都不劣于任何框架，代价是自己拥有一套 DOM 更新逻辑。选它做兜底而非首选，理由在 8.5-6 第 5 条。**替换的可行性由架构保证而非由承诺保证**：设计已写明「整个 ui crate 被换掉，一条策略都不会变」——策略住 kernel 与服务端，本 crate 只回答「算出来的东西怎么画」。故败诉线是一条真能走的路，不是一句安慰。

### 8.5-6 被否方案与理由

1. **Leptos**——判据一否决。维护者 2026-05 第一人称宣布减速维护、且明言不再做重要新开发；M2＝11.8。它在判据三（细粒度更新）上是全场最强的，但判据三排在末位，救不了判据一。**如果判据序反过来，结论会翻成 Leptos**——这是本结论对判据序最敏感的一处，写在明处。
2. **Sycamore**——判据一否决。M1＝332 天、M2≈57.8，是全场最接近「单人项目」的一个。
3. **Yew**——判据一勉强通过（多人社区、有发布节奏），判据二落后：唯一构建路径 Trunk 的稳定版已 470 天未动、0.22 长期停在 beta 且正在把 node 包下载能力加进来；判据三上虚拟 DOM 是全场最粗。三条叠加不敌 Dioxus。
4. **走 `dx` 的 Dioxus**——被 M5 否决，理由见 8.5-4 第 2 条。这是同一框架的两条路径，只否路径不否框架。
5. **不用框架（`web-sys` 直用）**——**未被否，降为兜底**。它在判据一与判据二上是最强的：地基本身没有第二层维护风险，构建链最短。三条理由使它不做首选：其一，钉版表与选型要求都写「前端框架」，选它需要先改那两处，属结构性变更；其二，九个 DOM 模块要自备带键列表更新、事件委派与焦点保持，这套东西的缺陷会长在 `web` crate 里由我们自己养；其三，它的优势恰好在框架失守时才兑现——所以它的正确位置是败诉线的落点，而不是起点。**若改取此案**，须同集修改钉版表与本节的选型要求，并按 ARCHITECTURE.md §13「结构变更须 verdict」留痕。
6. **egui／eframe 一类立即模式画布 UI**——被需求否决。`theme` 的权威表示是「OKLCH 源头常量 → **CSS 自定义属性**」，`xtask color` 与去色快照都建立在样式变量之上；画布 UI 没有 CSS 这一层，等于把颜色规则的机械可判性拆掉。另外视觉语言一章明确把无障碍角色与键盘顺序、字体回退与按需下载列为「浏览器已经提供、我们不再实现」的东西，画布 UI 会把这些重新变成我们的工作。

### 8.5-7 本结论未回答、留给后续卡的问题

- `wasm-bindgen-cli` 的具体钉版号与安装口令，随 S4.05 写入 AGENTS.md 环境前置节。
- `just build-web` 配方的确切内容（触碰 `justfile`，须携 `Verdict:` 尾注）。
- `web` 是否纳入 `apisync` 基线集（见 §3 末条）。
- 字体分片的切割方案与许可证随包，属 S4.08 之后的资源工程。

## 9 工作流程

（随 S4.05 填：从 `socket` 建连、握手校验、事件流入口，到 `app` 求值视图、DOM 应用与画布绘制的完整通路。）

## 10 实现逻辑

（随 S4.05–S4.08 逐卡填。）

## 11 边界枚举

（随卡填：握手失败／版本不配／断线重连时的事件缺口／空态三处／超长 Ledger 列表／画布缩放极值。）

## 12 错误处理

本 crate 不定义 AxCode；跨进程错误由 `channels::wire` 携 AxError 送达，界面按 three-part refusal 三段呈现（拒绝了什么／为什么／可执行的替代）。界面自身的失败（握手不配、连接断、渲染前置缺失）显式呈现，恒不静默降级。

## 13 依赖选型

| 依赖 | 用途 | 判据 |
|---|---|---|
| `dioxus` 0.7.10，`default-features = false`，`features = ["minimal", "web"]` | 九个 DOM 模块的组件与更新 | 见 §8.5 结论书（已裁决）。关 `asset` 与 `devtools` 两 feature 是硬规则，不是调优 |
| `wasm-bindgen`／`web-sys`／`js-sys` | 浏览器 API 绑定；WebSocket、通知、文档 | 工作区已钉；全部候选的共同地基。F2.02 退掉 `CanvasRenderingContext2d` 与 `HtmlCanvasElement` 两个 feature：等距城市改由 SVG 承担，绑定面随之收窄 |
| `channels`（本仓库） | Command／Query／Event 类型与编码 | 拓扑唯一上游（ARCHITECTURE §2） |

**不引**：任何 Markdown 解析器（服务端已渲染 HTML）；任何颜色空间转换库（浏览器做色域映射）；任何 UI 组件库（组件即样式，样式的权威是 `theme`）。

## 14 硬编码声明

`city_view::MARGIN`（F2.02）是一个估算值：楼名居中画在塔下，而本库量不了文本——宿主侧没有字体度量，向浏览器要一个则把一次测量放进了纯函数中间。故取一个够宽的常数，并用 `text-anchor: middle` 使溢出对称：估小了的后果是两端各差几个单位，而不是一侧被截。


`theme` 的七项源头常量是**故意的硬编码**，且是全 crate 唯一允许的颜色产地——「常量三源」把呈现类常量的产地定在 `web::theme`，`xtask color` 以它为权威扫描全仓颜色字面量。占位页 `assets/index.html` 中的三个十六进制值是待清理的过渡物（见 §4）。

## 15 影响面

- `crates/sprawling/build.rs`：嵌入源从占位页切到 wasm 构建产物（Stage 0 已预留，「只换复制源，别的不动」）。
- `justfile`：新增 `build-web` 配方（触碰受保护文件，须 `Verdict:` 尾注）。
- 根 `Cargo.toml`：`workspace.dependencies` 增 dioxus／wasm-bindgen／web-sys（同上，须 `Verdict:` 尾注）。
- `xtask color`：S4 起上线，数据面即本 crate 的 `theme` 常量（ARCHITECTURE §8 门表）。
- `xtask zerojs`：wasm-bindgen 产物不计（门表已写明豁免），但 `just build-web` 与 CI 步骤仍受该门约束。
- ARCHITECTURE §6 模块表 web 十行：随各卡从「未建」翻「已建」。

## 16 测试与约束

- 视图纯函数：同输入两次渲染输出等值（形状测试，不依赖浏览器）。
- `theme`：`xtask color` 六断言＋去色快照（色度系数置零重拍）。
- 端到端与视觉回归：无头浏览器驱动，S4.08 入 CI；对比度在渲染后的页面上实测。
- `city_view`：Stage 4 只验接口；位图回归属 P2。
- 约束：本 crate 零 `pub` trait（不在缝清单）；非测试代码遵守 C3 硬化全条。
- **门覆盖问题已作答（S4.05，实测而非推测）**：`just check` 跑的是 `cargo clippy --workspace`，而 `--workspace` **覆盖** `default-members`——`web` 确实被检查（实验：`touch crates/web/src/lib.rs` 后重跑，输出含 `Checking web`），nextest 同理跑它的测试。真正盖不到的只有两块：`cfg(target_arch = "wasm32")` 分支，以及 `channels` 关掉 `server` feature 的构建。**补门方式＝`just check-web`**（在 wasm32 目标上跑 clippy `-D warnings`），且本 crate 的全部判定逻辑故意写在 cfg 之外，使 host 侧的门与测试就能作证。

## 17 模型体验

零字节，因为本 crate 是人的界面，不进入任何 prefix。它对模型的唯一间接影响是：`alert` 与 `approval` 的呈现质量决定人多快作出裁决，而裁决延迟计入 Run 的墙钟时间，不计入 token。

## 18 文档同步

- ARCHITECTURE §6 模块表 web 十行状态；§6 接线台账「memory::index／hot／projection」「memory::attribution」「kernel::approval 应答面」三行的 S4 到期项。
- ARCHITECTURE §4 布局节末句「前端框架 Stage 4 结论书落定」——verdict 落定后回填具体结论。
- AGENTS.md 环境前置节：`wasm-bindgen-cli` 与 wasm32 目标；命令面表增 `just build-web` 行。
- 依赖钉版表「Rust 到 WebAssembly 的前端框架」行：verdict 落定后回填具体名字与版本。
- `crates/channels/channels-SPEC.md`：wire 类型是本 crate 的唯一上游，两份 SPEC 的 Command／Query／Event 表必须一致。
