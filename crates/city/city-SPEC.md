# city-SPEC.md

> crate：`city`。本 SPEC 先于代码存在；实现不多不少地遵守本文。
> 骨架：apostle-sdd 十七节；按模块分章、每章自足（ARCHITECTURE.md §5）。
> 动手前先读所用工具与依赖的**官方文档或官方 agent 指南**，再写本文的接口节。

## 1 需求拆解

本 crate 是城的空间与身份面，逐卡落地；每张卡先补齐 §8 的对应子节，再写代码：

| 卡 | 模块 | 这张卡回答的问题 | §8 |
|---|---|---|---|
| P1.06 | `resident` | 谁在跑这个 Run（身份从哪来、给 prefix 贡献什么、做过什么） | 8-1 |
| P1.08 | `policy` | 这栋楼里允许什么（confidential 三条、写域、出网） | 8-2 |
| **P2.01** | **`building`** | **一栋楼怎么被建出来，以及一个地址归哪栋楼管** | **8-3** |
| **P2.01** | **`config_layers`** | **三层配置住哪三个文件，又怎么求成一份 `FrozenConfig`** | **8-4** |
| **P2.02** | **`spine_files`** | **一栋楼开局有哪几份文档，一件活的 JOB.md 落在哪** | **8-5** |
| **P2.11** | **`schedule`** | **到点发车：谁在什么节奏上自己开始** | **8-6** |
| P3 | `archive`、`library` | 东西存哪里、怎么找回来 | 待写 |
| P4 | `office`、`wizard` | OFFICE.md；建城向导与搬家 | 待写 |

## 2 验收标准

一个 Resident 跨两个 Run 存活，且**两次 Run 的 resident 段字节相同**（`a_resident_crosses_two_runs_with_the_same_identity_segment`，bin 侧从 `model_called` 的 segments 哈希取证）；无 `URBANITE.md` 的地址落为 Ephemeral 且段文本明说这一点。

## 3 假设与歧义

「Resident 住哪」在本期取**地址即目录**：`<city>/<addr>/URBANITE.md`。Building 模板实例化（P2）落地后若改变目录形状，改的是 `urbanite_path` 一处。

## 4 现状分析

`city` 自 S0 起是空壳。本期第一次有代码。

## 5 权威信源

「空间、身份、历史」的语义（Resident 是身份、活跃 Run 才是开销；一个地址决定三件事）；`docs/templates/URBANITE.md`（这份文件长什么样）；ARCHITECTURE.md §12 模块图的 city 段。

## 6 命名统一

Identity（两态）｜Resident｜Ephemeral｜Dossier｜URBANITE.md。**不引入「persona」「角色」「档案」**——概念名一律英文原词，一个概念一个名字。

## 7 模块边界

**三件邻居的活，及它们各自的主人**（写「X 归 Y」而非「不做 X」）：

- 落盘与历史归 `memory`：本模块**读** `URBANITE.md`，写入与备份归 memory 与 checkpoint。
- Building 规则（confidential、写域、阅览室准入）归 `city::policy`（P1.08）：本模块只答「谁」，不答「他能做什么」。
- 身份的**呈现**归 `web`：Dossier 是数值，界面怎么画它是 web 的事。

## 8 接口先行

```rust
pub enum Identity { Resident(Resident), Ephemeral { addr: Address } }   // 穷尽两态
impl Identity {
    pub fn load(city_root: &Path, addr: &Address) -> Result<Identity, AxError>;
    pub fn segment_bytes(&self) -> Vec<u8>;   // prefix 的 resident 段
    pub fn addr(&self) -> &Address;
    pub fn who(&self) -> String;              // Ledger 记的 actor
}
pub struct Resident { /* addr、urbanite、digest —— 私有 */ }
impl Resident { pub fn addr(&self) -> &Address; pub fn digest(&self) -> B3Hash; }
pub fn urbanite_path(city_root: &Path, addr: &Address) -> PathBuf;

pub struct Dossier { /* 计数与位置 —— 私有 */ }        // 形状 7：投影
impl Dossier { pub fn apply(&mut self, who: &str, record: &EventRecord); pub fn is_live(&self) -> bool; /* … */ }
```

- **文件缺失不是错误，读不动才是**：多数房间没有常驻身份，故 `NotFound` 落为 `Ephemeral`；而一个**存在却读不出**的描述必须报错——静默降级成 Ephemeral 会让同一个地址在两次运行中读到两套指令，且没人看得出来。
- **Ephemeral 的段文本明说「你没有常驻身份」**：给它编一个性格，等于让一个用完即弃的执行体以为自己有历史。
- **Dossier 是投影不是文件**：做过什么已经在 Ledger 里；旁边再存一份摘要就是同一段过去的第二个说法。
- **`is_live` 由计数得出而非由标志位**：标志位需要有人清除，而崩溃之后没有人清除标志位。

### 8-2 city::policy（P1.08；形状 1 判定＋形状 2 值类型）

```rust
pub const BUILDING_FILE: &str = "BUILDING.md";
pub enum ModelPool { Any, LocalOnly }                 // 穷尽，不是 bool
pub struct BuildingRules { /* addr、policy、write_prefixes —— 私有 */ }
impl BuildingRules {
    pub fn policy(&self) -> &BuildingPolicy;          // 随每次模型调用出行
    pub fn model_pool(&self) -> ModelPool;
    pub fn write_domain(&self) -> Result<WriteDomain, AxError>;
}
pub fn load(city_root: &Path, addr: &Address) -> Result<BuildingRules, AxError>;
pub fn evaluate(addr: &Address, text: &str) -> Result<BuildingRules, AxError>;
pub fn building_path(city_root: &Path, addr: &Address) -> PathBuf;
```

- **confidential 三条各有其守处**：模型池锁本地由 `gateway::endpoint` **在会泄漏的那一端**拒（P1.08 同集补的判定：`req.policy.confidential` 即拒，携三段式）；写域止于本楼子树由 `write_domain()` 在构造点拒；数据可入不可出归出网门（P1.09）。**把兜底放在会出事的那一层**，路由错了仍然拦得住。
- **没有 BUILDING.md 是普通楼；有而不声明是错误**：把隐私设置的默认值悄悄取成宽松的那一边，正是这整个面存在的理由。拼写不是 `true`／`false` 同样拒——读起来像笔误的隐私设置不得解析成许可。
- **confidential 楼声明越界前缀＝拒而不裁剪**：静默裁剪会让文件说一套、城做另一套；拒绝会指出该改哪一行。
- **无声明写域时默认只写本楼**：一栋楼至少能写自己，且不多。
- **`review: true` 是楼级开关（P3.02）**：开则每个 Run 得一棵自己的 worktree，写的东西在别人检查并 merge 之前对楼不可见。**默认关**，与 confidential 的「不声明即错」相反——隐私的默认值不得惄悄取宽，而审查纪律的默认值不得惄悄取严：一个人派一个 Agent 去改一行字并盯着看，应当看得到文件变化。拼写不是 `true`／`false` 同样拒。
- **`## Egress` 列可达域名（P1.09）**：`BuildingRules::egress()` 交 `kernel::egress_target` 判定。**confidential 楼同时列域名＝矛盾，拒**——「数据可入不可出」是那个设置的含义，域名表写在它下面会逼读者自己去调和两句话。
- **今天的执行点与仍缺的执行点要分清**：provider 路径已被 `endpoint` 的 confidential 拒守住；Agent 自己发起的出网（exec 的 Program／Shell 臂、P4 浏览器）**没有可拦截处**，因为拦截需要 OS sandbox。判定已就位，拦截随 P4 落地——在那之前不要说「出网已管住」。

### 8-3 city::building（P2.01；形状 2 值类型＋一个实例化动作）

```rust
pub enum BuildingTemplate { Minimal, Confidential }   // 穷尽；新模板＝新臂
impl BuildingTemplate {
    pub fn parse(name: &str) -> Result<BuildingTemplate, AxError>;   // 不认即拒，且报出已知集
    pub fn name(&self) -> &'static str;
}
pub struct Building { /* addr —— 私有 */ }
impl Building {
    pub fn of(addr: &Address) -> Result<Building, AxError>;   // 一个地址归哪栋楼管
    pub fn addr(&self) -> &Address;
    pub fn root(&self, city_root: &Path) -> PathBuf;
    pub fn holds(&self, addr: &Address) -> bool;
}
pub fn create(city_root: &Path, addr: &Address, template: BuildingTemplate)
    -> Result<Building, AxError>;
pub fn created_payload(building: &Building, template: BuildingTemplate)
    -> Result<Payload, AxError>;
pub fn adopt(city_root: &Path, addr: &Address) -> Result<Building, AxError>;      // 整修卡 R1.03
pub fn adopted_payload(building: &Building) -> Result<Payload, AxError>;          // adopted: true
```

- **楼是顶层地址，房间不是楼**：`create` 拒多段地址（`lab/room1` 是 `lab` 里的一个房间）。嵌套楼会使「这个地址归谁管」多出一个答案，而 `Building::of` 取首段这件事今天已被写域、配置与上报对象三处消费。
- **reserved prefix 下建楼恒拒**：`.sprawling/` 是城自己的账与配置，它在一切写域之外；允许在它下面建楼，就是把一个写域开到账本上。判定用 `Address::is_reserved`，不在本模块重写前缀文法。
- **二次出生恒拒**：已有 `BUILDING.md` 即拒（同 `init` 拒第二次创世）。覆写会把一栋已在干活的楼的规则静默换掉，而那份规则可能写着 `confidential: true`。
- **模板字节来自 `docs/templates/BUILDING.md`（`include_str!`）**：人读的那份模板与城写出的那份必须是同一串字节，否则两份会各自漂。`Confidential` 与 `Minimal` 只差一行（`confidential:` 的值），且该差异由 `policy::evaluate` 读回来断言——换行成功与否不靠阅读，靠测试。
- **先落盘再产事件**：`building_created` 记的是已经发生的事。反过来的顺序会让历史声称一栋目录不存在的楼存在，而重放会把这个谎再说一遍。
- **只写不读的 payload**：`created_payload` 只有写面，因为今天没有读它的投影——`CityView` 的楼列表读盘（assembly 的 `read_spine`）。读面随第一个真正需要它的投影落地，不提前建。
- **adopt（R1.03，兑现「导入一个已有目录」这件事）**：收编一个已存在的目录为楼。复用 `create` 的全部围栏（房间拒、reserved 拒、二次出生拒），只多一条：**目录不存在即拒并指向 create**——收编不存在的东西是建造，两个动词不共用一个事实。Spine 文档恒不覆写（P2.02 既有约束），故被收编目录的 `Roadmap.md` 保持原主的字节；事件仍是 `building_created`，但 payload 携 `adopted: true`——历史不得声称它建造了它只是找到的东西。CLI 入口 `sprawling adopt <city> <addr>`；城外目录先由人搬入城内再收编，本体不做拷贝。

### 8-4 city::config_layers（P2.01；形状 1 判定／求值，兼任文件名权威）

```rust
pub const CONFIG_FILE: &str = "CONFIG.toml";
pub enum Layer { City, Building, Resident }        // 穷尽三级，与 kernel::LayeredValue 同形
pub fn path(city_root: &Path, addr: &Address, layer: Layer) -> Result<PathBuf, AxError>;
pub struct ConfigLayer { /* effort —— 私有 */ }
impl ConfigLayer {
    pub fn parse(text: &str) -> Result<ConfigLayer, AxError>;   // 纯函数，无 I/O
    pub fn effort(&self) -> Option<Effort>;
}
pub fn load(city_root: &Path, addr: &Address) -> Result<FrozenConfig, AxError>;
```

**门面上换名**（`lib.rs` 按能力组织，不按文件组织）：`load` 已归 `policy`，故本模块对外是 `city::load_config`；`path` 对外是 `city::config_path`；`building::create` 对外是 `city::create_building`，`created_payload` 对外是 `city::building_created_payload`（名字读起来就是它记的那个事件）。

- **文件名只有一份，层级由位置决定**：City 层住 `<city>/.sprawling/CONFIG.toml`（reserved prefix 内，因此任何 Resident 的写域都永远叠不上它——「Agent 改不了自己的配置」因此是判定而非推理）；Building 层住 `<city>/<building>/CONFIG.toml`；Resident 层住 `<city>/<addr>/CONFIG.toml`。三处同名，读者认一次就认得完。
- **地址就是楼时只有两级**：`addr` 与它的 building 相同时，下两级指向同一个文件，只读一次并放在 Building 级。同一份文件在两级各算一次不改变结果，却会让读者以为它能覆盖自己。
- **缺文件不是错，读不动才是**（同 `resident`）：未声明即每级 `None`，落到 `kernel::consts_policy` 的缺省；一份存在却读不出的配置报 `E_STORAGE_FATAL`。
- **不认的键即拒**（`deny_unknown_fields`）：静默忽略一个拼错的键，会产生「我设了 effort 而什么也没发生」这个无从诊断的状态。今天只受理 `[model] effort`；`[clock]` 等到它在真城里有消费者的那张卡再受理，在那之前写它得到的是一句拒绝而不是一份沉默。
- **梯子不在本模块重建**：下层胜上层由 `kernel::LayeredValue::resolve` 给，冻结由 `kernel::freeze` 给；本模块只回答「哪三份文件、怎么读」。一条规则一个权威。
- **effort 属 `FrozenConfig` 而非 `LiveConfig`**：改它会作废 message cache breakpoints，因此改动只影响下一个 Run（理由已写在 `kernel::config`，此处不重述只遵守）。

**R1.13 增（config_layers）**：`CONFIG.toml` 第三节 `[[mcp]]`（表数组），逐项三字段：`label`（解成 `kernel::ServerLabel`，非法即拒并报出是哪一份文件）、`command`（非空）、`args`（缺省空表）。两条口径：①**同一层内标签不得重复**——两个同名 server 会让同一个工具名同时指向两个进程，而那是一个路由错误而不是一个偏好；②整表上梯（下层写即替换上层全表），语义住 kernel-SPEC §8-22 的 R1.13 段，此处不复述。**不在本模块的事**：进程怎么起、起不来怎么办、confidential 楼凭什么拒——那三件全在装配层（sprawling-SPEC §8-4），本模块只回答「三份文件说了什么」。

**P4.02 增（config_layers）**：`CONFIG.toml` 第二节 `[sandbox]`，字段 `shell`（bool，默认 false）、`fuel`（整数，缺省取 `SANDBOX_FUEL_DEFAULT`）、`mounts`（相对 city root 的路径表，reserved prefix 在解析点即拒）。解析仍是「本版本不读的键即拒」——被写下却什么都不发生是唯一没人能诊断的状态。整节整值上梯，语义住 kernel-SPEC §8-22 的 P4.02 段，此处不复述。

### 8-5 city::spine_files（P2.02；形状 6 数据面＋落盘动作）

```rust
pub const ROADMAP_FILE: &str = "Roadmap.md";
pub const JOB_FILE: &str = "JOB.md";
pub const CITY_FILE: &str = "City.md";
pub(crate) const MEMO_FILE: &str = "Memo.md";      // 尚无外部读者
pub(crate) const HANDOFF_FILE: &str = "Handoff.md";

pub struct JobBrief<'a> { pub task: &'a str, pub goal: &'a str, pub budget: &'a str }
pub enum RunBrief { Job { text: String }, Principal }          // P6.03：穷尽两臂
pub(crate) fn lay_out(building_root: &Path, addr: &Address) -> Result<(), AxError>;  // 唯一调用方是 building::create
pub fn job_path(city_root: &Path, addr: &Address) -> PathBuf;
pub fn write_job(city_root: &Path, addr: &Address, brief: &JobBrief<'_>) -> Result<String, AxError>;
pub fn write_brief(city_root: &Path, addr: &Address, brief: &JobBrief<'_>) -> Result<RunBrief, AxError>;
pub fn handoff(city_root: &Path, building_addr: &Address) -> Option<String>;
pub fn norms(city_root: &Path, addr: &Address) -> Result<Vec<PathBuf>, AxError>;
```

- **四文档三写一不写**：`lay_out` 写 Roadmap／Memo／Handoff；`BUILDING.md` 归 `building::create`（它的含义归 `policy`）——同一份文件有两个写入者就是两个权威。
- **已存在的文档恒不覆写**：一栋已在干活的楼的计划不得因为又跑了一次建楼而回到空白。
- **模板的占位行不进新楼的 Roadmap**：`docs/templates/Roadmap.md` 里的两行 `Not started` 是给人看的例子；照抄进去，一栋新楼开局就有两件不存在的待办，而它们会进分母。实例化时删掉 Item 列为空的数据行，断言是「新楼的分母是 0」。
- **JOB.md 先落盘，再产 `run_started`**（模板第一行就这么写）；内容同时进 CAS，于是盘上那份是现场、CAS 那份是历史——Agent 改了 JOB.md 也不会使「当时派的是什么活」不可考。同一个房间再派一件活即覆写它（JOB.md 是本次会话的任务，不是档案）。
- **机器只填它知道的段**：Task／Goal／Budget 三段有事实就写；Background／Delivery 无事实则不写——写一个 `(未知)` 占位，只是让模型每回合读一遍没信息的行。
- **一次会话的 brief 只有两种，且由本次派活决定（P6.03）**：说得出 Goal 的就写 `JOB.md`（`RunBrief::Job`），说不出的就不写（`RunBrief::Principal`）。**判据选 Goal 而不选「盘上有没有 JOB.md」**：一个房间里上周留下的任务书仍在盘上，它可以被读，但不得冒充一次没人派任务的会话的 brief。Goal 是那份表单里唯一不可替代的一栏（什么时候停），它空着就等于告诉 Agent「停不停没定义」。
- **`handoff` 不把空白表单当交接件**：一张没填过的 `Handoff.md` 与一张填过的占同样的 prefix 字节而一个字的信息也不带。识别靠模板自己的括号提示行。
- **规范类 must-read 由 `norms` 给路径，不给 Locator**：Locator 需要 CAS 或 git oid，而 city 不认识落盘物（拓扑上也依赖不到 memory）。本模块答「哪几份是规范」，装配层把它们入 CAS 变成 Locator。这也是 must-read 最大失败模式的解：不让模型凭记忆重抄规范清单。

### 8-6 city::schedule（P2.11；形状 1 判定＋形状 6 数据面）

```rust
pub const SCHEDULE_FILE: &str = "SCHEDULE.toml";
pub enum Cadence { EveryMinutes(u64), DailyAt(u64), WeeklyAt(u64) }   // 穷尽
pub struct Entry { /* name、addr、task、goal、cadence —— 私有 */ }
pub struct Schedule { /* entries —— 私有 */ }
impl Schedule {
    pub fn parse(text: &str) -> Result<Schedule, AxError>;
    pub fn load(city_root: &Path) -> Result<Schedule, AxError>;
    pub fn due(&self, after: TimeMs, now: TimeMs) -> Vec<&Entry>;     // 时间只入参
}
```

- **一个窗口里每条最多回一次**：错过八小时的整点活欠一次运行而不是八次。窗口多宽由调用方定——bin 的 `tick` 把起点设在开机那一刻，于是**关机期间的活不在开机第一分钟补跑**；计时源是命令台的有限等待，故不新开线程也不往线格式加 `Tick`。
- **恒 UTC**：节奏按 epoch 分钟数整数运算，无历法依赖。关切时区是呈现面的事（ClockStamp），而一份依赖会动的时区库的日程会在重放时换一个时刻发车。
- **日历形状（day-of-month／month）明拒**：它们需要一部历法，而历法需要一个权威，城里还没有；拒词写明这一点，而不是近似成「每 30 天」。
- **一个 job 只许一个节奏**：写了两个即拒——排名它们等于替用户做一个他没做的决定。

### 8-7 city::watch（P4.08；形状 6 数据面＋形状 1 判定）

```rust
pub const WATCH_FILE: &str = "WATCH.toml";
pub struct Source { /* name、matches、addr、starts_work 私有 */ }
impl Source { pub fn name(&self) -> &str; pub fn matches(&self) -> &str; pub fn addr(&self) -> &Address;
              pub fn starts_work(&self) -> bool; pub fn building(&self) -> &str; }
pub enum Link { Live { since: TimeMs }, Down { since: TimeMs } }
pub struct Watch { /* sources 私有 */ }
impl Watch {
    pub fn parse(text: &str) -> Result<Watch, AxError>;
    pub fn load(city_root: &Path) -> Result<Watch, AxError>;
    pub fn listening(&self, standing: &[Address]) -> Vec<&Source>;
}
pub fn watch_path(city_root: &Path) -> PathBuf;
```

- **形状同 `schedule`**：盘上一张表、一个纯问题、答案是派活。差别在触发源：日程因时间流逝而响，城自己看得见；watch 因别处发生了事而响，城看不见。
- **本地恒不轮询〔用户裁定〕**：持连接的服务推过来，城只负责接。轮询是一份无人阅读的定时流量，也是第二份「谁先到」的权威。
- **`starts_work` 默认 false**：外来事到达本身不是花一次模型的理由。它为 true 时表示**人事先核过这个来源**——这与 `collab::triage` 的「污染件不自行开工」不冲突，两者答的是不同问题（详见 ARCHITECTURE.md §10 P3.07 的 P4.08 补记）。
- **楼拆即不再听**：`listening` 按现存楼过滤，而不是去改用户写的文件——文件是人的，城替人改文件就是在回答一个没人问的问题。
- **两条边都入 Ledger**：`Link` 的 Live 与 Down 各携时刻，于是「这栋楼何时没在听」是可读事实而非猜测。不做断线补投。

### 8-8 city::library（P3.12；形状 2 值类型＋形状 1 判定）

```rust
pub const LIBRARY_DIR: &str = "library";           // 住 reserved prefix 之下
pub struct Holding { pub name, pub section, pub disclosure, pub path }
pub struct Library { /* BTreeMap<(section, name), Holding> —— 私有 */ }
impl Library {
    pub fn scan(city_root: &Path) -> Result<Library, AxError>;   // 无库＝空库，不是错误
    pub fn all(&self) -> Vec<&Holding>;
    pub fn sections(&self) -> Vec<&str>;
    pub fn reading_room(&self, admitted: &[String]) -> Vec<&Holding>;
    pub fn missing(&self, admitted: &[String]) -> Vec<String>;
}
pub fn holding_address(holding: &Holding) -> Result<Address, AxError>;
```

- **中央库存与阅览室的分工是常驻上下文不随磁盘膨胀的唯一原因**：盘上躺一千件与本楼的常驻字节无关；进 catalog 的只有 `BUILDING.md` 的 `## Reading room` 列出的那几件。
- **库存住 reserved prefix**：Agent 读得到、写不了。否则一个 Agent 可以给自己发一件 SKILL，而那正是准入清单存在的理由。
- **一行式条目取作者写的第一行**，不生成摘要：摘要的摘要是消化产物，而消化产物默认可疑。
- **清单上没有的名字不进 catalog、也不报错**，只留一行诊断给写清单的人——承诺一件取不到的技能比它不在还糟。

### 8-10 city::wizard（P4.10；形状 1 判定＋形状 2 值类型）

```rust
pub struct CityPlan { /* dirs、first 私有 */ }
impl CityPlan {
    pub fn new(first: Option<(&str, &str)>) -> Result<CityPlan, AxError>;   // 一句指令的城
    pub fn dirs(&self) -> &[Address];
    pub fn first(&self) -> Option<&(Address, BuildingTemplate)>;
}
pub struct Relocation { pub from: Address, pub to: Address, pub crosses_building: bool }
pub fn relocate(from: &Address, to: &Address) -> Result<Relocation, AxError>;
```

- **两件都是判定，不是动作**：新城由什么构成、一次搬家蕴含什么，在这里以值给出；建目录与落事件归 bin。这个切分使「一句指令建一座城」可以在不建城的条件下被断言。
- **搬家恒不是改名**：一个 Address 同时决定写域、默认上下文与上报对象，所以搬家是换写域，而**历史留在它发生的地方**。一座会为了配合新地址而改写历史的城，「这件事是在哪儿做的」就没有答案了。
- **恒不得搬到楼根**：住在楼根等于从侧门拿到整栋楼的写域。
- **`city::office` 已并入 `config_layers` 并删行〔P4.10 定谳〕**：三层配置（City／Building／Resident）已是完整的梯子，OFFICE.md 没有任何一条自有规则，第四层只会成为「同一个设置在哪儿写」的第二个答案。

### 8-9 city::archive（P3.13；形状 2 值类型＋形状 7 投影）

```rust
pub enum Kind { Preference, Decision, Correction, Fact }   // 封闭四类
pub struct Entry { pub kind: Kind, pub day: u64, pub subject: String, pub at: PathBuf }
pub fn day_of(at: TimeMs) -> u64;                          // 时间只入参
pub fn file(city_root, building, kind, at, subject, body) -> Result<Entry, AxError>;
pub fn index(city_root, building) -> Result<Vec<Entry>, AxError>;   // 算出来的，不落盘
```

- **四类封闭**：第五类需要理由，而「它不属于前四类」正是让分类表烂掉的那个理由。不合的东西是笔记，笔记住人已经在读的文档里。
- **index 是算出来的**：存一份就是盘上内容的第二份账，而盘是真的那一份。删掉它没有东西可删——这正是投影该有的样子。
- **召回是结构化的**：按类与日期归档，循索引读**原文**。不做向量记忆；翻案条件写死——真实召回率 <90% 才重议。
- **日期取整天**：给人浏览用，精度高过问题所需只会招来没人打算做的比较。

### 8-14 一次会话选一次的思考强度（F2.16）

```rust
pub fn write_effort(city_root: &Path, addr: &Address, effort: Effort) -> Result<(), AxError>;
```

用户裁定（2026-08-24）：思考强度放在派活按钮旁边，因为**一次会话反正只选一次**。

- **写进那一层，而不是另存一份**：选择落到会话自己房间的 `CONFIG.toml`，由已有的 city → building → room 阶梯解析。第二个存处就是第二个答案。
- **只改 `[model] effort` 一个键**：文件里其它键是人写的，读出来、改一个值、写回去。文件读不动或解析不了就**拒绝**，不覆盖——一份本构建看不懂的配置不是可以随手盖掉的配置。
- **落点在 `<room>/.sprawling/`**（F2.08 起），所以这个房间里跑的 Run 读得到、改不了自己的档位。

### 8-13 city::room（F2.11；形状 1 判定 ＋ 一个实例化动作）

```rust
pub fn open(city_root: &Path, building: &Address, name: &SessionName) -> Result<Address, AxError>;
// crate 面：`pub use room::open as open_room;`——调用方读到的是 `city::open_room`，
// 因为裸的 `city::open` 在装配层里说不出开的是什么。
```

与 `city::building` 同形：一个判定加一个落盘动作，而不是一个长住的值。

- **同名加后缀，不复用**：`refactor`、`refactor-2`。复用会把两次只是共用一个词的会话放进同一套文件，而那正是本模块要消掉的缺陷。「继续上一次会话」是向它已有的房间地址派活，由界面给出选项，而不是靠拼写撞对。
- **`create_dir` 而非先问后建**：问与答是一个操作，于是同一毫秒里的两次派活不会被同时告知「这个名字空着」。
- **999 个后缀封顶**：一个写错的循环应该停下来，而不是把盘写满。

### 8-12 一个 skill 只有一个家：楼自己的书架（F2.10）

```rust
pub struct Holding { pub name, pub section, pub disclosure, pub path, pub addr: Address }
impl Library {
    pub fn scan(city_root: &Path, building: Option<&Address>) -> Result<Library, AxError>;
}
// 删除：pub fn holding_address(&Holding) -> Result<Address, AxError>
```

用户要的是「一栋楼就是一个可以 cd 过去的工作区」；已记录的理由是「住户只读得到存货，不得给自己进货」。F2.08 之后两者不再冲突：楼自己的书架放在 `<building>/.sprawling/skills/<section>/<name>.md`，在楼目录里、在写域之外。

- **近的书架盖远的**：同名同 section 时楼的那本胜出。这不是新规则，而是 `config_layers` 已有的那一条（低层胜，高层是回落）在书架上的同一个实例。
- **城的书架只放两栋以上共用的**：一个 skill 只有一个家。代价是找一本 skill 要看两处，换到的是一栋楼拷走就带着它自己的本事。
- **`holding_address` 删掉**：它从 section＋name 拼回一个城级路径，而 `Holding` 本来就握着 `path`——两个权威，且在两层书架下其中一个必然答错。地址改为扫盘时算一次、存在 `Holding::addr` 里。

### 8-11 楼的治理字节搬进它自己的保留子树（F2.09；沉淀一处路径权威）

```rust
pub fn building_path(city_root, addr) -> PathBuf;   // <building>/.sprawling/BUILDING.md
pub fn config_layers::path(city_root, addr, layer) -> Result<PathBuf, AxError>;
// 三层统一为 <scope>/.sprawling/CONFIG.toml；city 层因此不再是特例
```

承 F2.08：不变式已经立起来了，本卡把字节搬到它后面。

- **三层一个表达式**：`path()` 先算出 scope 目录（城根、楼根、房间目录），再一律 `.join(RESERVED_PREFIX).join(CONFIG_FILE)`。原先 City 层写死了 `city_root.join(RESERVED_PREFIX)` 而另两层没有，那个不对称正是洞口。
- **旧城必须报错，不得静默降级**：`policy::load` 把「BUILDING.md 不存在」当作默认策略（`confidential: false`）。搬完之后，一座旧城的楼就会从「机密」静默变成「不机密」——所以新位置缺失而旧位置存在时**拒绝**，`E_CONFIG_INVALID`，recovery 直接拿出要执行的那一步移动。这不是兼容适配层（它不读旧文件），是一道不让静默降级发生的门。
- **楼页仍然看得见 BUILDING.md**：`assembly` 组楼页答案时跳过一切点头目录（房间枚举因此也不会把 `.sprawling` 当房间），故 BUILDING.md 改为**按路径显式读一次**再入档。人在界面上看得到、改得了；楼里的 agent 读得到、写不了。
- **默认写域仍是整栋楼，这是故意的**：一次 Run 为它那栋楼产出一份楼级产物是正常的；把默认收紧到房间会把那件事一并禁掉，而洞口在于治理文件的位置，不在于写域的宽窄。

## 8.5 两个设计

**A（选中）：`Identity` 两态枚举**。调用方拿到的东西自己会说自己是谁，段字节由它给出。
**B（落选）：`Option<Resident>`**。少一个类型，但每个调用方都要自己决定「None 时该给 prefix 什么」——那是一条散落在每个调用点的策略，且第一个忘记写的人会得到一个空的 resident 段。翻案条件：出现第三种身份（例如 P2 的代表某人的临时身份），届时枚举照常扩，Option 则无法扩。

**P2.01 的两个设计（配置文件名）**

**A（选中）：三层同名 `CONFIG.toml`，层级由位置决定**。读者记一个名字；把一份配置放错层是一个**位置**错误，而位置在目录树里看得见。
**B（落选）：每层各起一名**（`CITY.toml`／`BUILDING.toml`／`RESIDENT.toml`）。文件名自带层级信息，代价是三个名字要同时被记住，且放错层变成一个**拼写**错误——拼写错误要靠逐字比对才看得出。翻案条件：出现同一目录下共存两级配置的需求（例如一栋楼的默认与它自己作为房间的默认同处），届时位置不再能区分层级。

## 9 工作流程

bin `RunWorker::dispatch` → `Identity::load(city_root, addr)` → `segment_bytes()` 进 `FrozenPrefix` 的 resident 槽 → `who()` 成为 Ledger 的 actor。

## 10 实现逻辑

纯 std：一次 `fs::read` 与一次 `B3Hash::digest`。`urbanite_path` 按地址分段 push，故不做字符串拼接，Windows 上也无分隔符问题。

## 11 边界枚举

无文件（→Ephemeral）｜文件存在但无读权限（→报错）｜空文件（→Resident，段为空字节；空描述是作者的选择，不是缺陷）｜地址含多段（`lab/room1`，逐段 push）。

## 12 错误处理

`E_STORAGE_FATAL`（读不动一个存在的描述）：不可定义掉——文件系统权限是外部世界的事实，而静默降级是被明拒的替代。

## 13 依赖选型

只依赖 `kernel`（拓扑硬约束）＋ std。dev 依赖 `tempfile`。

P2.01 增两件，均已在 workspace 钉版（不新增版本权威）：`toml` 与 `serde`（derive）。理由：三层配置的格式是 TOML，而 `toml` 已被 `xtask` 消费（budgets.toml／lexicon.toml）；解析走 serde derive 加 `deny_unknown_fields`，使「写错的键」在反序列化那一刻失败。手写一个 TOML 子集解析器是可行的另一条路，已落选：它会把一个已有权威的格式变成本库自己的私有变体。

## 14 硬编码声明

`URBANITE_FILE = "URBANITE.md"`（公开常量）；Ephemeral 段文本（私有常量，改它即改一个 Ephemeral 读到的第一句话）。

P2.01：`CONFIG_FILE = "CONFIG.toml"`（公开常量；三层同名，理由见 §8.5）；新楼的 `BUILDING.md` 字节不写在代码里，而是 `include_str!("../../../docs/templates/BUILDING.md")`——它的权威是那份模板，路径写错在编译期就会被堵住；`Confidential` 模板对该字串做一处行替换（`confidential: false` → `true`），替换是否真的生效由 `policy::evaluate` 读回来断言。

## 15 影响面

bin 装配层的 prefix 组装（P1.06 同集改）；`docs/templates/URBANITE.md` 是这份文件的模板，两者改动须同期。

P2.01 波及 bin 装配层三处：`run_command` 增 `CreateBuilding` 臂；`dispatch` 里的本地函数 `building_of` **删除**，改用 `city::Building::of`（一条规则一个权威）；`CallShape.effort` 不再恒为 `None`，改由 `config_layers::load` 供给——接线台账里「Effort 值的生产者待接」那一行到此为止。

## 16 测试与约束

三条：身份两态各一条；**段字节跨两次加载稳定**；Dossier 只计本人的 Run 且跨 Run 累加。bin 侧另有一条端到端断言（两次 Dispatch 的 resident 段哈希相同、run 段不同）。

P2.01 七条：新建的楼被 `policy::load` 读回且 confidential 模板真的锁本地模型池｜二次出生恒拒｜reserved prefix 下建楼恒拒｜房间地址建楼恒拒且拒词指出该建哪栋｜下层配置盖上层｜不认的键即拒｜**写在 `CONFIG.toml` 里的 effort 出现在真实出线请求体里**（bin 侧端到端，假 provider 录下请求体）。

## 17 模型体验

P2.01 入窗零字节：`CONFIG.toml` 改的是请求字段（effort），不是模型读到的文本；新楼的 `BUILDING.md` 则经 `city::policy` 进判定面，不逐字入窗。

resident 段是模型每回合都读到的四段之一。`URBANITE.md` 建议 30 行以内：长的描述不会让 Resident 更能干，只会让每个回合更贵——这句话写在模板里，因为模板在场即教学。

## 18 文档同步

新增模块随卡登记 ARCHITECTURE.md §6 与接线台账；`Dossier` 的生产消费者（Resident 视图）到位时更新台账行。

P2.01 同集四处：§6 模块表两行翻 `已建`；§6 接线台账的 `kernel::config`（freeze 面）与 Effort 两行改成已接线；`xtask/api-baselines/city.txt` 随公开面同集重算；`docs/templates/BUILDING.md` 从此是被实例化的那串字节，改它即改新楼的第一句话。
