// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Every user-visible word this client can be read in, and the two
//! languages it says them in (web-SPEC.md section 8-37).
//!
//! One entry holds both languages, so a translation cannot be missing:
//! there is no arm to forget, only a struct with two fields that must
//! both be filled for the file to compile. The Chinese follows
//! `README.zh-CN.md` — 城 / 楼 / 房间 / 会话 / Ledger — because a
//! product that names one concept two ways has two concepts.

use std::fmt;

/// A language this interface can be read in.
///
/// Two, not a table of locales: each one costs a translation of every
/// phrase, and a half-translated language is worse than an untranslated
/// one because it reads as a defect rather than as a choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    En,
    Zh,
}

impl Lang {
    /// The tag this language is stored and requested by.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh",
        }
    }

    /// What this language calls itself. Never translated: a person
    /// looking for their own language looks for its own name.
    #[must_use]
    pub fn endonym(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Zh => "中文",
        }
    }

    /// The language a browser asking for `tag` should be given.
    ///
    /// Prefix-matched, because a browser says `zh-CN` or `zh-Hans-CN`
    /// and means Chinese. Anything else is English, which is what this
    /// client is written in.
    #[must_use]
    pub fn of(tag: &str) -> Lang {
        if tag.to_ascii_lowercase().starts_with("zh") {
            Lang::Zh
        } else {
            Lang::En
        }
    }

    /// Both, in the order a switch offers them.
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Zh];
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

/// Where a chosen language is remembered between visits.
#[cfg(target_arch = "wasm32")]
const STORED_UNDER: &str = "sprawling.lang";

/// The language this page opens in: what was chosen here before, and
/// failing that what the browser itself asks for.
///
/// Outside a browser there is nothing to ask, and the answer is the
/// language this client is written in.
#[must_use]
pub fn preferred() -> Lang {
    #[cfg(target_arch = "wasm32")]
    {
        let window = web_sys::window();
        let stored = window
            .as_ref()
            .and_then(|window| window.local_storage().ok().flatten())
            .and_then(|store| store.get_item(STORED_UNDER).ok().flatten());
        if let Some(tag) = stored {
            return Lang::of(&tag);
        }
        if let Some(asked) = window.and_then(|window| window.navigator().language()) {
            return Lang::of(&asked);
        }
    }
    Lang::En
}

/// Remembers a choice, so a person chooses once rather than on every
/// visit. A browser that refuses storage is not an error: the choice
/// still holds for this page's life.
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(
        unused_variables,
        reason = "there is nowhere to remember a choice outside a browser"
    )
)]
pub fn remember(lang: Lang) {
    #[cfg(target_arch = "wasm32")]
    if let Some(store) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = store.set_item(STORED_UNDER, lang.tag());
    }
}

/// One phrase in both languages. A struct rather than two arms of a
/// match: a field cannot be left out the way an arm can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Phrase {
    pub en: &'static str,
    pub zh: &'static str,
}

impl Phrase {
    #[must_use]
    pub fn in_lang(self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.en,
            Lang::Zh => self.zh,
        }
    }
}

/// Everything this client says that a person reads and that is not a
/// name, a number or something the city itself wrote.
///
/// Exhaustive and non-`non_exhaustive`: adding a variant fails to
/// compile until [`phrase`] says it in both languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    // The left-hand navigation.
    CityNoBuildings,
    NavTheRecord,
    NavCity,
    NavCost,
    NavSettings,
    // The five phases a session is read in (web::phase is the only producer).
    PhaseRunning,
    PhaseWaiting,
    PhaseFrozen,
    PhaseCancelled,
    PhaseHalted,
    // The six destinations of the v0.0.3 information architecture.
    NavSessions,
    NavWaiting,
    // The composer: the one box that starts work.
    ComposerTitle,
    ComposerScope,
    ComposerExample,
    ComposerSendTo,
    ComposerAs,
    ComposerThink,
    ComposerKeys,
    ComposerSource,
    ComposerRoomFor,
    ComposerModeFor,
    ComposerEffortFor,
    // The sessions list.
    SessionsScope,
    SessionsSource,
    SessionsEnded,
    SessionsEndedScope,
    SessionsEndedSource,
    SessionsNothingYet,
    SessionsNothingWhat,
    SessionsTurnCount,
    SessionsUnpriced,
    SessionsCityScope,
    // One session: the object page.
    SessionAllSessions,
    SessionTurnOrdinal,
    SessionSpentIs,
    SessionAtGate,
    SessionNoGate,
    SessionContextUnknown,
    SessionContextScope,
    SessionHandoffAt,
    SessionHandoffNone,
    SessionHandoffJust,
    SessionTabTurns,
    SessionTabChanges,
    SessionTabCost,
    SessionTabDocs,
    SessionTabPrompt,
    // What the run was actually sent, and what it was allowed to reach.
    PromptTitle,
    PromptScope,
    PromptSource,
    PromptAtTurn,
    PromptBytes,
    PromptNone,
    PromptNoneWhat,
    PromptSkillsTitle,
    PromptSkillsScope,
    PromptSkillsSource,
    PromptNoSkills,
    PromptNoSkillsWhat,
    PromptSkillFirst,
    PromptSkillSame,
    PromptSkillChanged,
    SessionScope,
    SessionSource,
    SessionUnknown,
    SessionUnknownWhat,
    // What waits on a person.
    WaitingScope,
    WaitingSource,
    WaitingNothing,
    WaitingNothingWhat,
    WaitingFrozenHeading,
    // The record.
    RecordScope,
    RecordLensLedger,
    RecordLensArchive,
    RecordLensBin,
    // The three-rung readiness ladder (empty states, not wizard steps).
    FirstNoModelTitle,
    FirstNoModelScope,
    FirstNoModelStatus,
    FirstNoModelWhat,
    FirstNoModelWay,
    FirstNoModelSubscription,
    FirstNoModelSource,
    FirstNoBuildingTitle,
    FirstNoBuildingScope,
    FirstNoBuildingStatus,
    FirstNoBuildingWhat,
    FirstNoBuildingWay,
    FirstNoBuildingSource,
    FirstDispatchTitle,
    FirstDispatchScope,
    FirstDispatchKeys,
    FirstDispatchSource,
    // The city's own standing, at the foot of the nav.
    CityRunning,
    CityRunningIdle,
    PursuitTitle,
    PursuitScope,
    PursuitSource,
    PursuitEmpty,
    PursuitEmptyWhat,
    PursuitSet,
    PursuitPause,
    PursuitResume,
    PursuitClear,
    PursuitGoalLabel,
    AutonomyTitle,
    AutonomyScope,
    AutonomySource,
    AutonomyOwner,
    AutonomyDeferred,
    CityStopped,
    CityUnwell,
    CountRunning,
    CountWaiting,
    CountBuildings,
    // The control surface: the one place work is started.
    EffortInherited,
    EffortLow,
    EffortMedium,
    EffortHigh,
    EffortXHigh,
    EffortMax,
    DispatchSend,
    HaltCity,
    ReleaseCity,
    CancelLastRun,
    VitalsRecords,
    VitalsSignals,
    VitalsDiscards,
    VitalsAsking,
    ProviderUnknown,
    ProviderHealthy,
    ProviderDegraded,
    ProviderLost,
    AskingWhatItHolds,
    LineToolCalled,
    LineToolResult,
    LineModelCalled,
    LineModelReturned,
    LineSteered,
    LineGateDenied,
    LineApprovalRequested,
    LineRunFrozen,
    LiveNothingSinceConnected,
    LiveOneSession,
    LiveEveryRun,
    LiveScope,
    LiveSource,
    LiveEverything,
    LiveRunId,
    LiveFollowEnd,
    LiveDropped,
    LiveNoRunYet,
    LiveNothingSince,
    LiveNoRunYetWhat,
    LiveNothingSinceWhat,
    LivePickASession,
    LiveSteerPlaceholder,
    LiveSteerSend,
    LiveForkFrom,
    LiveNothingToBranch,
    LiveInterventionNote,
    CityScope,
    CityTowerNote,
    CitySource,
    CityStanding,
    CityStageLabel,
    CityBuildingNamePlaceholder,
    CityRaiseBuilding,
    CityNoBuildingsWhat,
    ReadIt,
    CityMoveLeft,
    CityMoveRight,
    CityMoveUp,
    CityMoveDown,
    CityFit,
    CityReadWhat,
    CityWhatShouldHappen,
    CityWhatCountsAsDone,
    CitySendWorkHere,
    CityClearSelection,
    BuildingAsking,
    BuildingAskingWhat,
    BuildingTitle,
    BuildingScope,
    BuildingSource,
    BuildingStartHere,
    BuildingNoRooms,
    BuildingUnreadableRow,
    BuildingArchiveTab,
    BuildingAskingRoom,
    BuildingAskingRoomWhat,
    BuildingRoomEmpty,
    BuildingRoomEmptyWhat,
    BuildingSignalFrom,
    BuildingNothingFiled,
    BuildingNothingFiledWhat,
    BuildingTruncated,
    BuildingNoDocument,
    ApprovalNothingWaiting,
    ApprovalTitle,
    ApprovalScope,
    ApprovalSource,
    ApprovalNoneEscalated,
    ApprovalNoneEscalatedWhat,
    ApprovalTainted,
    BinRestoreCheckpoint,
    BinRestoreStored,
    BinRebuild,
    BinNoDescription,
    BinAsking,
    BinAskingWhat,
    BinNothingDiscarded,
    BinTitle,
    BinScope,
    BinSource,
    BinNoneYet,
    BinNoneYetWhat,
    BinAlreadyRestored,
    BinRollback,
    BinRollbackNote,
    LedgerNothingSaid,
    LedgerTitle,
    LedgerScope,
    LedgerSource,
    LedgerWhoActed,
    LedgerAnyPartOfName,
    LedgerKindOfEvent,
    LedgerEveryKind,
    LedgerNoMatch,
    LedgerNothingArrived,
    LedgerFilterNote,
    LedgerFirstLineNote,
    LedgerNewestMatching,
    LedgerTakeThisPage,
    ArchiveTitle,
    ArchiveHits,
    ArchiveScope,
    LedgerNewer,
    LedgerOlder,
    LedgerSkipped,
    BuildingWaitingCount,
    SettingsInterfaceTitle,
    SettingsInterfaceScope,
    SettingsInterfaceSource,
    SettingsInterfaceFaces,
    SettingsInterfaceContent,
    ArchiveSource,
    ArchiveSearchFor,
    ArchiveWordPlaceholder,
    ArchiveSearchButton,
    ArchiveNoSearch,
    ArchiveNoSearchWhat,
    ArchiveAskingFiled,
    ArchiveListWhenArrives,
    ArchiveNothingFiled,
    ArchiveNothingFiledWhat,
    ArchiveFiledLately,
    CostAskingSpent,
    CostAskingSpentWhat,
    CostNothingSpent,
    CostUnpricedTitle,
    CostWhereMoneyWent,
    CostUnpricedScope,
    CostScope,
    CostSource,
    CostNoneBilled,
    CostNoneBilledWhat,
    CostCutEmpty,
    ProgressNoPlan,
    TurnNumber,
    TurnTools,
    TurnNoTools,
    TurnWaiting,
    TurnAnswered,
    TurnFailed,
    TurnTokens,
    TurnStopped,
    TurnOutput,
    TurnOutputCut,
    NoteWaiting,
    NoteFenced,
    NoteArrived,
    NoteDiscarded,
    ChangedFiles,
    ChangedNothing,
    ChangedAdded,
    ChangedModified,
    ChangedDeleted,
    ChangedRenamed,
    ChangedBinary,
    LiveEveryEvent,
    PalettePlaceholder,
    PaletteNothing,
    PaletteNothingWhat,
    PaletteKindPage,
    PaletteKindBuilding,
    PaletteKindSession,
    KeysTitle,
    KeysScope,
    KeysPalette,
    KeysCompose,
    KeysDismiss,
    KeysGo,
    KeysShow,
    AlertCannot,
    AlertNoRecovery,
    AlertAwaitingApproval,
    AlertRunFrozen,
    AlertProviderTrouble,
    AlertRefused,
    AlertSomethingWaiting,
    AlertRunStopped,
    AlertProviderNotAnswering,
    StatusNoCity,
    StatusProvider,
    StatusNothingSpent,
    StatusAwaitingYou,
    StatusAwaitingAndUnreadable,
    StatusUsedNoPrice,
    StatusSpent,
    StatusSpentSomeUnpriced,
    RouteNoSuchPage,
    RouteNoSuchPageRecovery,
    SettingsAttachIt,
    SettingsNeedsName,
    SettingsNeedsUrl,
    SettingsUrlNotSafe,
    SettingsNeedsDialect,
    SettingsOnThisMachine,
    SettingsOffThisMachine,
    SettingsWithCredential,
    SettingsNoCredential,
    SettingsMainConsequence,
    SettingsDigestConsequence,
    SettingsUnknownConsequence,
    SettingsStoredAs,
    SettingsUseThisModel,
    SettingsPickProvider,
    SettingsPickModel,
    SettingsPickJob,
    SettingsModelNotServed,
    SettingsAsking,
    SettingsAskingWhat,
    SettingsDispatchable,
    SettingsNotDispatchable,
    SettingsScope,
    SettingsSource,
    SettingsNoProvider,
    SettingsNoProviderWhat,
    SettingsAttachProvider,
    SettingsCallIt,
    SettingsNamePlaceholder,
    SettingsBaseUrl,
    SettingsUrlHint,
    SettingsWhichWire,
    SettingsKey,
    SettingsKeyPlaceholder,
    SettingsKeyHint,
    SettingsPutKeyInVault,
    SettingsAttachThisProvider,
    DropAction,
    DropRecovery,
    DropNotAPlace,
    DropUnreadable,
    DropHere,
    BuildingReachTab,
    BuildingShell,
    BuildingFuel,
    BuildingMounts,
    BuildingServers,
    BuildingServersHint,
    BuildingSaveReach,
    SettingsAskWhatItServes,
    SettingsServes,
    SettingsAdmitAll,
    SettingsSignIn,
    SettingsProvider,
    SettingsStartLogin,
    SettingsNoLoginWaiting,
    SettingsOpenApproveePaste,
    SettingsCodeLabel,
    SettingsPasteHere,
    SettingsFinishLogin,
    SettingsChooseModelHeading,
    SettingsWhichProvider,
    SettingsWhichModel,
    SettingsForWhichJob,
    SettingsWhatFor,
    SettingsPointJobAtModel,
    SettingsWhatEachModelIsFor,
    SettingsWhatIsAttached,
    SettingsModelCount,
    SettingsReadItAgain,
    // The plan tree, laid out by state (V3.23).
    BoardTitle,
    BoardScope,
    BoardSource,
    BoardReady,
    BoardWaiting,
    BoardWorking,
    BoardBlocked,
    BoardDone,
    BoardEmpty,
    BoardEmptyWhat,
    BoardStuck,
    BoardStuckScope,
    BoardStuckSource,
    BoardWaitingBehind,
    BoardWaitsFor,
    BuildingPlanTab,
    // The settings page: the language switch itself.
    SettingsLanguage,
    SettingsLanguageScope,
    SettingsLanguageSource,
}

/// The one table. Both languages of one phrase sit on one line, where a
/// reader can see them disagree.
#[must_use]
pub fn phrase(msg: Msg) -> Phrase {
    match msg {
        Msg::CityNoBuildings => Phrase {
            en: "no building has been raised yet",
            zh: "还没有立起一栋楼",
        },
        Msg::NavTheRecord => Phrase {
            en: "the record",
            zh: "记录",
        },
        // Names the question the group answers, and must not repeat the
        // one page under it: a heading reading `settings` above an item
        // reading `settings` is a heading carrying nothing. What is
        // actually true of everything here is that it belongs to this
        // installation rather than to the city - the key vault is on this
        // disk, and so is the reading language.
        Msg::NavCity => Phrase {
            en: "city",
            zh: "城市",
        },
        // `Run` is the identifier; 会话 is what a reader of Claude Code or
        // Codex already calls one, and section 8.1 keeps one word per
        // concept in each column.
        Msg::NavCost => Phrase {
            en: "cost",
            zh: "成本",
        },
        Msg::NavSettings => Phrase {
            en: "settings",
            zh: "设置",
        },
        Msg::PhaseRunning => Phrase {
            en: "running",
            zh: "在跑",
        },
        Msg::PhaseWaiting => Phrase {
            en: "needs you",
            zh: "等你",
        },
        Msg::PhaseFrozen => Phrase {
            en: "frozen",
            zh: "已冻结",
        },
        Msg::PhaseCancelled => Phrase {
            en: "you stopped it",
            zh: "你取消了它",
        },
        Msg::PhaseHalted => Phrase {
            en: "the city stopped",
            zh: "城停了",
        },
        Msg::NavSessions => Phrase {
            en: "what's happening",
            zh: "在做的事",
        },
        Msg::NavWaiting => Phrase {
            en: "waiting on you",
            zh: "等我的",
        },
        Msg::ComposerTitle => Phrase {
            en: "what needs doing?",
            zh: "要做什么？",
        },
        Msg::ComposerScope => Phrase {
            en: "one sentence is enough. where it goes and how it runs are both in the line below.",
            zh: "写一句话就够。送到哪、算什么模式，下面那句里都说了。",
        },
        Msg::ComposerExample => Phrase {
            en: "measure every read path in the ledger, find the slowest one, and write down the numbers you got",
            zh: "把账本的读取路径量一遍，找出最慢的那一处，并写下测出来的数",
        },
        Msg::ComposerSendTo => Phrase {
            en: "send it to {room}",
            zh: "送到 {room}",
        },
        Msg::ComposerAs => Phrase {
            en: "as {mode}",
            zh: "以 {mode}",
        },
        Msg::ComposerThink => Phrase {
            en: "think {effort}",
            zh: "想 {effort}",
        },
        Msg::ComposerKeys => Phrase {
            en: "Enter sends it, Shift+Enter makes a new line.",
            zh: "Enter 送出，Shift+Enter 换行。",
        },
        Msg::ComposerSource => Phrase {
            en: "a dotted word is one this city guessed, from where you sent work last and what this city defaults to; a solid one you set yourself. Click any word to change it.",
            zh: "带点线的词是这座城猜的，来自你上一次派活与城的默认；实线的是你自己设的。点任意一个词可以改它。",
        },
        Msg::ComposerRoomFor => Phrase {
            en: "which room",
            zh: "送到哪间房",
        },
        Msg::ComposerModeFor => Phrase {
            en: "which mode",
            zh: "算什么模式",
        },
        Msg::ComposerEffortFor => Phrase {
            en: "how hard to think",
            zh: "想多深",
        },
        Msg::SessionsScope => Phrase {
            en: "sessions that are running, or stopped waiting on you. What has ended is in the section below.",
            zh: "在跑的与在等你的会话（一间房里的一条工作线）。已经结束的在下面一节。",
        },
        Msg::SessionsSource => Phrase {
            en: "from the event stream, through seq {seq}.",
            zh: "来自事件流，最后一条 seq {seq}。",
        },
        Msg::SessionsEnded => Phrase {
            en: "what has ended",
            zh: "已经结束的",
        },
        Msg::SessionsEndedScope => Phrase {
            en: "the last eight. Any of them can be branched from any one of its turns.",
            zh: "最近八件。每一件都可以从任意一轮分出一支接着做。",
        },
        Msg::SessionsEndedSource => Phrase {
            en: "the same event stream; frozen and cancelled are its two endings.",
            zh: "同一条事件流，冻结与取消两种收尾。",
        },
        Msg::SessionsNothingYet => Phrase {
            en: "nothing has been sent out yet.",
            zh: "还没有派出过活。",
        },
        Msg::SessionsNothingWhat => Phrase {
            en: "the box above is where work starts; what you send appears here as a row.",
            zh: "上面那个框就是活的起点；送出去的东西会在这里变成一行。",
        },
        Msg::SessionsTurnCount => Phrase {
            en: "{n} turns",
            zh: "{n} 轮",
        },
        Msg::SessionsUnpriced => Phrase {
            en: "not priced",
            zh: "未报价",
        },
        Msg::SessionsCityScope => Phrase {
            en: "which buildings are busy. One block, not a page of its own.",
            zh: "哪几栋楼在忙。它是一块，不是一页。",
        },
        Msg::SessionAllSessions => Phrase {
            en: "all sessions",
            zh: "全部会话",
        },
        Msg::SessionTurnOrdinal => Phrase {
            en: "turn {n}",
            zh: "第 {n} 轮",
        },
        Msg::SessionSpentIs => Phrase {
            en: "spent {amount}",
            zh: "花了 {amount}",
        },
        Msg::SessionAtGate => Phrase {
            en: "stopped at the {gate} gate",
            zh: "停在 {gate} 这道门",
        },
        Msg::SessionNoGate => Phrase {
            en: "no gate is holding it",
            zh: "没有门拦着它",
        },
        Msg::SessionContextUnknown => Phrase {
            en: "context ——",
            zh: "上下文 ——",
        },
        Msg::SessionContextScope => Phrase {
            en: "context left is the one of these four the wire cannot carry yet: the city measures it, and no query returns it.",
            zh: "上下文余量是这四格里唯一线上还没有的：城里量得到，没有任何查询答得出它。",
        },
        Msg::SessionHandoffAt => Phrase {
            en: "handoff written {n} turns ago",
            zh: "Handoff {n} 轮前写过",
        },
        Msg::SessionHandoffNone => Phrase {
            en: "no handoff written yet",
            zh: "还没写过 Handoff",
        },
        Msg::SessionHandoffJust => Phrase {
            en: "handoff written this turn",
            zh: "Handoff 这一轮刚写过",
        },
        Msg::SessionTabTurns => Phrase {
            en: "turns",
            zh: "轮次",
        },
        Msg::SessionTabChanges => Phrase {
            en: "changes",
            zh: "改动",
        },
        Msg::SessionTabCost => Phrase {
            en: "spend",
            zh: "花费",
        },
        Msg::SessionTabDocs => Phrase {
            en: "documents",
            zh: "文档",
        },
        Msg::SessionTabPrompt => Phrase {
            en: "prompt",
            zh: "提示词",
        },
        Msg::PromptTitle => Phrase {
            en: "this is what went to the model",
            zh: "发给模型的就是这些",
        },
        Msg::PromptScope => Phrase {
            en: "the four frozen blocks of the prefix, in the order they are cached. The \
                 words city, building, resident and run are the ledger's own.",
            zh: "前缀的四个冻结块，按缓存的顺序排。city、building、resident、run 四个词是账本自己的写法。",
        },
        Msg::PromptSource => Phrase {
            en: "read out of this session's own prompt_assembled records; nothing here is \
                 hashed a second time.",
            zh: "从这条会话自己的 prompt_assembled 记录里读出；这里没有任何东西被重新算一遍哈希。",
        },
        Msg::PromptAtTurn => Phrase {
            en: "assembled at turn {n}",
            zh: "第 {n} 轮组装的",
        },
        Msg::PromptBytes => Phrase {
            en: "{n} bytes",
            zh: "{n} 字节",
        },
        Msg::PromptNone => Phrase {
            en: "this session has not assembled a prompt yet.",
            zh: "这条会话还没有组装过提示词。",
        },
        Msg::PromptNoneWhat => Phrase {
            en: "the first one is written when the first turn opens.",
            zh: "第一份在第一轮开始时落账。",
        },
        Msg::PromptSkillsTitle => Phrase {
            en: "what this session was allowed to open",
            zh: "这条会话被允许打开的东西",
        },
        Msg::PromptSkillsScope => Phrase {
            en: "the reading room this building admits, and what each document hashed to \
                 when the shelf was read.",
            zh: "这栋楼准进的阅览室，以及扫架时每份文档的哈希。",
        },
        Msg::PromptSkillsSource => Phrase {
            en: "pinned in run_started, and compared with the newest earlier session that \
                 was given the same name.",
            zh: "钉在 run_started 里，与拿到同一个名字的上一条会话相比。",
        },
        Msg::PromptNoSkills => Phrase {
            en: "this building admits no skills.",
            zh: "这栋楼一个技能都没准进。",
        },
        Msg::PromptNoSkillsWhat => Phrase {
            en: "the reading room is a list a person writes in BUILDING.md; a name not on \
                 the shelves is left out rather than promised.",
            zh: "阅览室是人在 BUILDING.md 里写的一张单子；单上有而架上没有的名字会被略去，而不是先应承下来。",
        },
        Msg::PromptSkillFirst => Phrase {
            en: "first time this city recorded it",
            zh: "这座城第一次记下它",
        },
        Msg::PromptSkillSame => Phrase {
            en: "same bytes as last time",
            zh: "与上次字节相同",
        },
        Msg::PromptSkillChanged => Phrase {
            en: "changed since last time, when it was {was}",
            zh: "自上次以来变了，当时是 {was}",
        },
        Msg::SessionScope => Phrase {
            en: "one work line in one room. Everything it did is under the tabs.",
            zh: "一间房里的一条工作线。它做过的一切在下面的标签里。",
        },
        Msg::SessionSource => Phrase {
            en: "from this session's own slice of the event stream.",
            zh: "来自这条会话在事件流里的那一段。",
        },
        Msg::SessionUnknown => Phrase {
            en: "this city has no session at that address.",
            zh: "这座城在那个地址上没有会话。",
        },
        Msg::SessionUnknownWhat => Phrase {
            en: "it may have been opened by another city, or the link may predate this one.",
            zh: "它可能属于另一座城，也可能这条链接比这座城还老。",
        },
        Msg::WaitingScope => Phrase {
            en: "everything that cannot move until you answer.",
            zh: "所有在你回答之前动不了的东西。",
        },
        Msg::WaitingSource => Phrase {
            en: "approvals from the queue, frozen sessions from the stream.",
            zh: "审批来自队列，冻结的会话来自事件流。",
        },
        Msg::WaitingNothing => Phrase {
            en: "nothing is waiting on you.",
            zh: "没有东西在等你。",
        },
        Msg::WaitingNothingWhat => Phrase {
            en: "approvals, gates raised to a person, and a stopped city would all appear here.",
            zh: "审批、升给人的门、停住的城，都会出现在这里。",
        },
        Msg::WaitingFrozenHeading => Phrase {
            en: "stopped, and not by you",
            zh: "停住了，而不是你停的",
        },
        Msg::RecordScope => Phrase {
            en: "what this city wrote down, in three lenses over one history.",
            zh: "这座城写下来的东西，一段历史的三个镜头。",
        },
        Msg::RecordLensLedger => Phrase {
            en: "the ledger",
            zh: "账本",
        },
        Msg::RecordLensArchive => Phrase {
            en: "the archive",
            zh: "归档",
        },
        Msg::RecordLensBin => Phrase {
            en: "the recycle bin",
            zh: "回收站",
        },
        Msg::FirstNoModelTitle => Phrase {
            en: "this city cannot send work out yet",
            zh: "这座城还派不出活",
        },
        Msg::FirstNoModelScope => Phrase {
            en: "the first step is one thing: tell it which model to ask. A key on this machine never leaves this machine.",
            zh: "第一步只有一件事：告诉它去问哪个模型。这台机器上的密钥不会离开这台机器。",
        },
        Msg::FirstNoModelStatus => Phrase {
            en: "no model answers for {tag}.",
            zh: "没有模型回答 {tag} 这个用途。",
        },
        Msg::FirstNoModelWhat => Phrase {
            en: "once one does, you write a sentence and it goes out to be done.",
            zh: "接上之后，你写一句话，它就能派出去做。",
        },
        Msg::FirstNoModelWay => Phrase {
            en: "attach a model service",
            zh: "去接一个模型服务",
        },
        Msg::FirstNoModelSubscription => Phrase {
            en: "I have a subscription, not an API key",
            zh: "我用的是订阅，不是 API key",
        },
        Msg::FirstNoModelSource => Phrase {
            en: "read from the endpoints registered on this machine and the jobs each one answers for.",
            zh: "读的是这台机器上已注册的 endpoint 与它们各自答应的用途。",
        },
        Msg::FirstNoBuildingTitle => Phrase {
            en: "there is nowhere to put work yet",
            zh: "还没有地方放活",
        },
        Msg::FirstNoBuildingScope => Phrase {
            en: "a building is one line of business, and on disk it is one folder; sending work out opens a room inside it.",
            zh: "楼（一条业务线，磁盘上就是一个文件夹）是活落脚的地方；一次派活会在楼里开一间房。",
        },
        Msg::FirstNoBuildingStatus => Phrase {
            en: "this city has no buildings at all.",
            zh: "这座城一栋楼都没有。",
        },
        Msg::FirstNoBuildingWhat => Phrase {
            en: "make one, say {example}; after that every piece of work lives in a room under it, named by what you called the work.",
            zh: "建一栋，比如 {example}；之后每一件活都住在它下面的一间房里，房间名就是你给这件活取的名字。",
        },
        Msg::FirstNoBuildingWay => Phrase {
            en: "make the first building",
            zh: "建第一栋楼",
        },
        Msg::FirstNoBuildingSource => Phrase {
            en: "read from the buildings already under the city root.",
            zh: "读的是城根目录下已有的楼。",
        },
        Msg::FirstDispatchTitle => Phrase {
            en: "send out the first piece of work",
            zh: "派出第一件活",
        },
        Msg::FirstDispatchScope => Phrase {
            en: "one sentence is enough; the rest is in the line below. What happens goes into the ledger — this city's whole history, append-only, verifiable offline.",
            zh: "写一句话就够，剩下的下面那句里都说了。做完之后，它会把过程完整记进账本（这座城的全部历史，只能追加，可以离线验）。",
        },
        Msg::FirstDispatchKeys => Phrase {
            en: "Enter sends it, Shift+Enter makes a new line. The grey sentence is the size one piece of work should be.",
            zh: "Enter 送出，Shift+Enter 换行。灰框里那句就是一件活该有的大小。",
        },
        Msg::FirstDispatchSource => Phrase {
            en: "nothing has been sent out yet, so all three words are guesses: the only building there is, mode build, and this city's default depth.",
            zh: "还没有派过活，所以三个词全是猜的：楼取城里唯一的一栋，模式取 build，思考深度取城的默认。",
        },
        Msg::PursuitTitle => Phrase {
            en: "standing goal",
            zh: "持续目标",
        },
        Msg::PursuitScope => Phrase {
            en: "this building, until the work runs out",
            zh: "这栋楼，直到活干完",
        },
        Msg::PursuitSource => Phrase {
            en: "folded from pursuit_changed",
            zh: "折自 pursuit_changed",
        },
        Msg::PursuitEmpty => Phrase {
            en: "this building is not pursuing anything.",
            zh: "这栋楼没有在追任何目标。",
        },
        Msg::PursuitEmptyWhat => Phrase {
            en: "set a goal and the city hands out ready work by itself, stopping when nothing is ready and nothing is in flight.",
            zh: "设一个目标，这座城就会自己派出就绪的活；就绪集空且无在途时停下。",
        },
        Msg::PursuitSet => Phrase {
            en: "pursue this",
            zh: "开始追",
        },
        Msg::PursuitPause => Phrase {
            en: "pause",
            zh: "暂停",
        },
        Msg::PursuitResume => Phrase {
            en: "resume",
            zh: "继续",
        },
        Msg::PursuitClear => Phrase {
            en: "stop pursuing",
            zh: "停止追",
        },
        Msg::PursuitGoalLabel => Phrase {
            en: "what this building is working towards",
            zh: "这栋楼要达成什么",
        },
        Msg::AutonomyTitle => Phrase {
            en: "who answers",
            zh: "谁来答",
        },
        Msg::AutonomyScope => Phrase {
            en: "approvals raised in this building",
            zh: "这栋楼里提出的审批",
        },
        Msg::AutonomySource => Phrase {
            en: "the rung this building states",
            zh: "这栋楼自己写下的那一档",
        },
        Msg::AutonomyOwner => Phrase {
            en: "a person answers",
            zh: "由人来答",
        },
        Msg::AutonomyDeferred => Phrase {
            en: "hold until somebody asks",
            zh: "先搁着，等人来问",
        },
        Msg::CityRunning => Phrase {
            en: "this city is running.",
            zh: "这座城在运转。",
        },
        Msg::CityRunningIdle => Phrase {
            en: "this city is running, with nothing to do.",
            zh: "这座城在运转，只是还没有活可做。",
        },
        Msg::CityStopped => Phrase {
            en: "this city is stopped.",
            zh: "这座城停住了。",
        },
        Msg::CityUnwell => Phrase {
            en: "no model service is attached",
            zh: "还没有接上模型服务",
        },
        Msg::CountRunning => Phrase {
            en: "{n} running",
            zh: "{n} 在跑",
        },
        Msg::CountWaiting => Phrase {
            en: "{n} waiting on you",
            zh: "{n} 等你",
        },
        Msg::CountBuildings => Phrase {
            en: "{n} buildings",
            zh: "{n} 栋楼",
        },
        Msg::EffortInherited => Phrase {
            en: "as the city says",
            zh: "跟随全城设定",
        },
        Msg::EffortLow => Phrase {
            en: "low",
            zh: "低",
        },
        Msg::EffortMedium => Phrase {
            en: "medium",
            zh: "中",
        },
        Msg::EffortHigh => Phrase {
            en: "high",
            zh: "高",
        },
        Msg::EffortXHigh => Phrase {
            en: "very high",
            zh: "很高",
        },
        Msg::EffortMax => Phrase {
            en: "as much as it has",
            zh: "拉满",
        },
        Msg::DispatchSend => Phrase {
            en: "send it",
            zh: "派出去",
        },
        Msg::HaltCity => Phrase {
            en: "stop the city",
            zh: "停城",
        },
        Msg::ReleaseCity => Phrase {
            en: "let the city go on",
            zh: "放行",
        },
        Msg::CancelLastRun => Phrase {
            en: "stop this session",
            zh: "停下这条会话",
        },
        Msg::VitalsRecords => Phrase {
            en: "records in the Ledger",
            zh: "条 Ledger 记录",
        },
        Msg::VitalsSignals => Phrase {
            en: "signals waiting in rooms",
            zh: "条信号在房间里等着",
        },
        Msg::VitalsDiscards => Phrase {
            en: "discarded, not taken back",
            zh: "件已丢弃且未取回",
        },
        Msg::VitalsAsking => Phrase {
            en: "asking the city how large it is",
            zh: "正在问这座城有多大",
        },
        Msg::ProviderUnknown => Phrase {
            en: "not connected",
            zh: "未接入",
        },
        Msg::ProviderHealthy => Phrase {
            en: "working",
            zh: "正常",
        },
        Msg::ProviderDegraded => Phrase {
            en: "unstable",
            zh: "不稳定",
        },
        Msg::ProviderLost => Phrase {
            en: "unreachable",
            zh: "连不上",
        },
        Msg::AskingWhatItHolds => Phrase {
            en: "asking the city what it holds",
            zh: "正在问这座城里有什么",
        },
        Msg::LineToolCalled => Phrase {
            en: "{who} calls a tool",
            zh: "{who} 调用了一个工具",
        },
        Msg::LineToolResult => Phrase {
            en: "{who} reads the result",
            zh: "{who} 读到了结果",
        },
        Msg::LineModelCalled => Phrase {
            en: "{who} asks the model",
            zh: "{who} 去问模型",
        },
        Msg::LineModelReturned => Phrase {
            en: "the model answers {who}",
            zh: "模型答复了 {who}",
        },
        Msg::LineSteered => Phrase {
            en: "{who} steers",
            zh: "{who} 说了一句",
        },
        Msg::LineGateDenied => Phrase {
            en: "a gate refused {who}",
            zh: "一道门拒绝了 {who}",
        },
        Msg::LineApprovalRequested => Phrase {
            en: "{who} needs a person",
            zh: "{who} 需要人来定",
        },
        Msg::LineRunFrozen => Phrase {
            en: "{who} is frozen",
            zh: "{who} 已冻结",
        },
        Msg::LiveNothingSinceConnected => Phrase {
            en: "nothing has happened since this page connected",
            zh: "本页连上之后什么都没发生",
        },
        Msg::LiveOneSession => Phrase {
            en: "one session, as it happens",
            zh: "一个会话，实时",
        },
        Msg::LiveEveryRun => Phrase {
            en: "every run in this city, as it happens",
            zh: "这座城里的每一个会话，实时",
        },
        Msg::LiveScope => Phrase {
            en: "a bounded window: the figure counts the lines held here, and a line that leaves the window has not left the Ledger",
            zh: "一个有上限的窗口：数字是这里留着的行数，滑出窗口的行并没有离开 Ledger",
        },
        Msg::LiveSource => Phrase {
            en: "the live event stream, folded one record at a time. Nothing here is re-asked or polled - the same fold the server does, running in this page.",
            zh: "实时事件流，一条一条地折。这里不重问也不轮询——服务端做的那个折叠，在本页里跑一遍。",
        },
        Msg::LiveEverything => Phrase {
            en: "everything",
            zh: "全部",
        },
        Msg::LiveRunId => Phrase {
            en: "run {id}",
            zh: "会话 {id}",
        },
        Msg::LiveFollowEnd => Phrase {
            en: "follow the end",
            zh: "跟到最新",
        },
        Msg::LiveDropped => Phrase {
            en: "{dropped} earlier line(s) left this window; the ledger still has them",
            zh: "{dropped} 条更早的行已滑出这个窗口；Ledger 里还留着",
        },
        Msg::LiveNoRunYet => Phrase {
            en: "no run has reported here yet",
            zh: "还没有会话在这里报过",
        },
        Msg::LiveNothingSince => Phrase {
            en: "nothing has happened here since this page connected",
            zh: "本页连上之后这里什么都没发生",
        },
        Msg::LiveNoRunYetWhat => Phrase {
            en: "this window holds what arrives from now on, so a run that finished before you opened this page is in the Ledger rather than here. Send work from the bar below and every turn it takes appears as it happens.",
            zh: "这个窗口只装从现在起到达的东西，所以在你打开本页之前就结束的会话在 Ledger 里而不在这里。从下面那条栏派活，它的每一轮都会实时出现。",
        },
        Msg::LiveNothingSinceWhat => Phrase {
            en: "this window holds what arrived after the page connected. Earlier lines are in the Ledger, which the record page reads.",
            zh: "这个窗口装的是页面连上之后到达的行。更早的在 Ledger 里，记录页读的就是它。",
        },
        Msg::LivePickASession => Phrase {
            en: "pick a session above to speak into it; what you send arrives at its next safe point",
            zh: "先在上面点一个会话才能对它说话；你发的话会在它下一个安全点到达",
        },
        Msg::LiveSteerPlaceholder => Phrase {
            en: "say something into this run",
            zh: "对这个会话说一句",
        },
        Msg::LiveSteerSend => Phrase {
            en: "send at the next safe point",
            zh: "在下一个安全点送达",
        },
        Msg::LiveForkFrom => Phrase {
            en: "branch a new run from step {seq}",
            zh: "从第 {seq} 步分出一个新会话",
        },
        Msg::LiveNothingToBranch => Phrase {
            en: "nothing to branch from yet",
            zh: "还没有可分叉的地方",
        },
        Msg::LiveInterventionNote => Phrase {
            en: "A branch records where it came from and does not start working by itself. Taking over answers for this run from here; what it already did is not undone.",
            zh: "分叉只记录它从哪来，不会自己开始干活。接手是由你从这里替它作答；它已经做过的事不会被撤销。",
        },
        Msg::CityScope => Phrase {
            en: "its buildings, how much work each has taken on, and which of them are busy right now",
            zh: "它的楼、每栋接下了多少活、此刻哪几栋忙着",
        },
        Msg::CityTowerNote => Phrase {
            en: "a tower is as tall as the work its plan took on and lit as far up as that work is done; a lit window is a run in flight right now",
            zh: "楼有多高取决于它的计划接下了多少活，亮到多高取决于做完了多少；一扇亮着的窗是此刻正在跑的一个会话",
        },
        Msg::CitySource => Phrase {
            en: "where the buildings stand comes from one query, asked when this page opened; which of them are lit is folded from the event stream, record by record, and is never polled",
            zh: "楼站在哪里来自打开本页时问的一次查询；哪几栋亮着是从事件流一条一条折出来的，从不轮询",
        },
        Msg::CityStanding => Phrase {
            en: "{raised} building(s), {busy} run(s) in flight",
            zh: "{raised} 栋楼，{busy} 个会话在跑",
        },
        Msg::CityStageLabel => Phrase {
            en: "the buildings of this city",
            zh: "这座城的楼",
        },
        Msg::CityBuildingNamePlaceholder => Phrase {
            en: "a name for the building",
            zh: "给这栋楼起个名字",
        },
        Msg::CityRaiseBuilding => Phrase {
            en: "raise a building",
            zh: "盖一栋楼",
        },
        Msg::CityNoBuildingsWhat => Phrase {
            en: "a building is one line of business: its own rules, its own plan, its own archive, and the rooms work happens in. Raise one above and it appears here with the ground under it.",
            zh: "一栋楼是一条业务线：自己的规则、自己的计划、自己的归档，以及干活的那些房间。在上面盖一栋，它就会连同脚下的地一起出现在这里。",
        },
        Msg::ReadIt => Phrase {
            en: "read it",
            zh: "读它",
        },
        Msg::CityMoveLeft => Phrase {
            en: "move left",
            zh: "左移",
        },
        Msg::CityMoveRight => Phrase {
            en: "move right",
            zh: "右移",
        },
        Msg::CityMoveUp => Phrase {
            en: "move up",
            zh: "上移",
        },
        Msg::CityMoveDown => Phrase {
            en: "move down",
            zh: "下移",
        },
        Msg::CityFit => Phrase {
            en: "fit",
            zh: "复位",
        },
        Msg::CityReadWhat => Phrase {
            en: "read what {id} has written down",
            zh: "读 {id} 写下的东西",
        },
        Msg::CityWhatShouldHappen => Phrase {
            en: "what should happen here",
            zh: "这里该发生什么",
        },
        Msg::CityWhatCountsAsDone => Phrase {
            en: "what counts as done",
            zh: "什么算做完",
        },
        Msg::CitySendWorkHere => Phrase {
            en: "send work here",
            zh: "往这里派活",
        },
        Msg::CityClearSelection => Phrase {
            en: "clear selection",
            zh: "取消选中",
        },
        Msg::BuildingAsking => Phrase {
            en: "asking {addr} what it has written down",
            zh: "正在问 {addr} 写下了什么",
        },
        Msg::BuildingAskingWhat => Phrase {
            en: "its plan, its decisions, its handoff and its archive are files on disk, read when this page asks",
            zh: "它的计划、决定、交接与归档都是盘上的文件，本页问的时候才读",
        },
        Msg::BuildingTitle => Phrase {
            en: "what {addr} has written down",
            zh: "{addr} 写下的东西",
        },
        Msg::BuildingScope => Phrase {
            en: "the documents this building keeps, its archive, and what waits in each of its rooms. The figure counts rooms; this page reads and never writes.",
            zh: "这栋楼留着的文档、它的归档，以及每个房间里等着的东西。数字数的是房间；本页只读不写。",
        },
        Msg::BuildingSource => Phrase {
            en: "the building's own directory on disk, read when this page asked. A room's queue is folded from the Ledger, so looking at it is not taking from it.",
            zh: "这栋楼在盘上的目录，本页问的时候读的。房间的队列是从 Ledger 折出来的，所以看一眼不等于取走。",
        },
        Msg::BuildingStartHere => Phrase {
            en: "start a session here",
            zh: "在这里开一个会话",
        },
        Msg::BuildingNoRooms => Phrase {
            en: "no rooms yet - work here has not been given one",
            zh: "还没有房间——这里还没派过活",
        },
        Msg::BuildingUnreadableRow => Phrase {
            en: "this plan row could not be read - {problem}",
            zh: "这一行计划读不了——{problem}",
        },
        Msg::BuildingArchiveTab => Phrase {
            en: "archive ({count})",
            zh: "归档（{count}）",
        },
        Msg::BuildingAskingRoom => Phrase {
            en: "asking what waits in {room}",
            zh: "正在问 {room} 里等着什么",
        },
        Msg::BuildingAskingRoomWhat => Phrase {
            en: "until that answer arrives this page cannot say whether the room is empty, and will not guess",
            zh: "答案到达之前本页说不出这个房间是不是空的，也不会猜",
        },
        Msg::BuildingRoomEmpty => Phrase {
            en: "nothing waits in {room}",
            zh: "{room} 里没有东西等着",
        },
        Msg::BuildingRoomEmptyWhat => Phrase {
            en: "another resident can leave a signal here, and a run in this room pulls it at its next safe point. Looking is not taking.",
            zh: "别的居民可以在这里留一条信号，这个房间里的会话会在下一个安全点取走。看一眼不等于取走。",
        },
        Msg::BuildingSignalFrom => Phrase {
            en: "from {who}",
            zh: "来自 {who}",
        },
        Msg::BuildingNothingFiled => Phrase {
            en: "nothing has been filed in this building",
            zh: "这栋楼里还没有归档过任何东西",
        },
        Msg::BuildingNothingFiledWhat => Phrase {
            en: "a resident files what it settled and does not want to work out twice. What is here is what the next run in this building is told before it starts.",
            zh: "居民把已经定下、不想再推一遍的东西归档。这里的东西就是下一个会话开工前会被告知的东西。",
        },
        Msg::BuildingTruncated => Phrase {
            en: " - shown up to the page's limit; the file on disk is longer",
            zh: " — 只显示到本页的上限；盘上的文件更长",
        },
        Msg::BuildingNoDocument => Phrase {
            en: "a building keeps five documents at most, and this one has not been written. The tabs above are the ones that exist.",
            zh: "一栋楼最多留五份文档，这一份还没写过。上面的页签是已经存在的那几份。",
        },
        Msg::ApprovalNothingWaiting => Phrase {
            en: "nothing is waiting for you",
            zh: "没有东西在等你",
        },
        Msg::ApprovalTitle => Phrase {
            en: "what the city stopped to ask you",
            zh: "城停下来问你的事",
        },
        Msg::ApprovalScope => Phrase {
            en: "one row per action a gate escalated rather than decided; grouped where one answer can settle several",
            zh: "门没有自己决定而上交的动作各一行；一个回答能了结几件的就并成一组",
        },
        Msg::ApprovalSource => Phrase {
            en: "the approval queue as the city holds it, plus every approval_requested that has arrived since this page connected",
            zh: "城里那份待批队列，加上本页连上之后到达的每一条 approval_requested",
        },
        Msg::ApprovalNoneEscalated => Phrase {
            en: "no gate has escalated anything",
            zh: "没有门上交过任何事",
        },
        Msg::ApprovalNoneEscalatedWhat => Phrase {
            en: "a run reaches a person only when a door refuses to decide by itself - a write outside its domain, a discard with no way back, an action a policy has not yet settled. Until then work runs without asking.",
            zh: "只有门自己不敢决定时才会找到人——写到域外、删了没有回头路、策略还没定过的动作。在那之前活自己往下跑，不问。",
        },
        Msg::ApprovalTainted => Phrase {
            en: "this one began with someone else's text: answered alone, and no policy can waive it",
            zh: "这一条起于别人的文本：只能单独回答，任何策略都免不了它",
        },
        Msg::BinRestoreCheckpoint => Phrase {
            en: "restore from the checkpoint at {at}",
            zh: "从 {at} 那个 checkpoint 恢复",
        },
        Msg::BinRestoreStored => Phrase {
            en: "restore the stored copy at {at}",
            zh: "恢复 {at} 处存着的副本",
        },
        Msg::BinRebuild => Phrase {
            en: "rebuild it: {how}",
            zh: "重建：{how}",
        },
        Msg::BinNoDescription => Phrase {
            en: "this build cannot describe how to restore it; the Ledger records the plan",
            zh: "本构建说不出怎么恢复它；Ledger 里记着那个方案",
        },
        Msg::BinAsking => Phrase {
            en: "asking the city what it discarded",
            zh: "正在问这座城丢弃过什么",
        },
        Msg::BinAskingWhat => Phrase {
            en: "the list appears when the answer arrives",
            zh: "答案到了列表就出现",
        },
        Msg::BinNothingDiscarded => Phrase {
            en: "nothing has been discarded",
            zh: "没有丢弃过任何东西",
        },
        Msg::BinTitle => Phrase {
            en: "what was deleted, and the way back to each of it",
            zh: "被删掉的东西，以及每一件的回头路",
        },
        Msg::BinScope => Phrase {
            en: "the newest first; the figure counts what has not been taken back yet, and rows that already came back stay listed as evidence that a return path works",
            zh: "最新的在前；数字数的是还没取回的，已经取回的行仍然列着，作为回头路管用的证据",
        },
        Msg::BinSource => Phrase {
            en: "folded from the Ledger's file_discarded and discard_restored records; the way back is the Restoration the discard was constructed with",
            zh: "折自 Ledger 的 file_discarded 与 discard_restored；回头路就是当初构造这次丢弃时带的那个 Restoration",
        },
        Msg::BinNoneYet => Phrase {
            en: "no run has discarded anything",
            zh: "还没有会话丢弃过东西",
        },
        Msg::BinNoneYetWhat => Phrase {
            en: "a deletion in this city cannot be constructed without a way back, so anything that disappears from a worktree lands here carrying the checkpoint or the content address it can be fetched from.",
            zh: "这座城里的删除没有回头路就构造不出来，所以从工作树里消失的东西都会落到这里，带着它的 checkpoint 或者内容地址。",
        },

        Msg::BinAlreadyRestored => Phrase {
            en: "already restored",
            zh: "已经取回",
        },
        Msg::BinRollback => Phrase {
            en: "put the whole worktree back to that checkpoint",
            zh: "把整个工作树拉回那个 checkpoint",
        },
        Msg::BinRollbackNote => Phrase {
            en: "A checkpoint row can be rolled back, and that puts the whole worktree back to that point - not this one file. Rows whose way back is a content address or a rebuild have no button, because the wire has no command that restores one file, and a button that did nothing would be worse than the sentence beside it.",
            zh: "带 checkpoint 的行可以回滚，那会把整个工作树拉回那一点——不是单独这一个文件。回头路是内容地址或重建说明的行不给按钮，因为线格式上没有恢复单个文件的命令，而一个按下去没反应的按钮比旁边这句话更糟。",
        },
        Msg::LedgerNothingSaid => Phrase {
            en: "the city has not said anything since this page connected",
            zh: "本页连上之后这座城什么都没说过",
        },
        Msg::LedgerTitle => Phrase {
            en: "what this city has done, newest first",
            zh: "这座城做过的事，最新在前",
        },
        Msg::LedgerScope => Phrase {
            en: "every kind of event, unless the two filters below narrow it; fifty rows to a page",
            zh: "每一种事件，除非下面两个筛选把它收窄；一页五十行",
        },
        Msg::LedgerSource => Phrase {
            en: "the live event stream since this page connected. The Ledger on disk holds the rest, and `sprawling replay` verifies the chain over all of it - including the part this page never saw.",
            zh: "本页连上之后的实时事件流。其余的在盘上的 Ledger 里，`sprawling replay` 会对整条链验一遍——包括本页从未见过的那一段。",
        },
        Msg::LedgerWhoActed => Phrase {
            en: "who acted",
            zh: "谁做的",
        },
        Msg::LedgerAnyPartOfName => Phrase {
            en: "any part of a name",
            zh: "名字的任意一段",
        },
        Msg::LedgerKindOfEvent => Phrase {
            en: "kind of event",
            zh: "事件种类",
        },
        Msg::LedgerEveryKind => Phrase {
            en: "every kind",
            zh: "所有种类",
        },
        Msg::LedgerNoMatch => Phrase {
            en: "no record here matches that filter",
            zh: "这里没有记录匹配这个筛选",
        },
        Msg::LedgerNothingArrived => Phrase {
            en: "nothing has arrived since this page connected",
            zh: "本页连上之后什么都没到达",
        },
        Msg::LedgerFilterNote => Phrase {
            en: "the filter is a view over what this page holds, not over the Ledger. Widen it, or clear both fields to see everything that has arrived.",
            zh: "筛选是对本页所持内容的一个视图，不是对 Ledger 的。放宽它，或者清空两个字段，就能看到已经到达的全部。",
        },
        Msg::LedgerFirstLineNote => Phrase {
            en: "every effect in this city becomes an event before it happens, so the first line appears the moment work is sent from the bar below.",
            zh: "这座城里的每一个效果都先成为事件再发生，所以从下面那条栏派出活的一瞬间，第一行就会出现。",
        },
        Msg::LedgerNewestMatching => Phrase {
            en: "the newest {rows} that match",
            zh: "匹配中最新的 {rows} 条",
        },
        Msg::LedgerTakeThisPage => Phrase {
            en: "take this page",
            zh: "导出这一页",
        },
        Msg::ArchiveTitle => Phrase {
            en: "what this city has written down",
            zh: "这座城写下过的东西",
        },
        Msg::LedgerNewer => Phrase {
            en: "newer",
            zh: "更新",
        },
        Msg::LedgerOlder => Phrase {
            en: "older",
            zh: "更旧",
        },
        Msg::LedgerSkipped => Phrase {
            en: "{skipped} newer record(s) skipped",
            zh: "略过了 {skipped} 条更新的记录",
        },
        Msg::BuildingWaitingCount => Phrase {
            en: "{count} waiting. Looking is not taking: a signal leaves this queue when a run pulls it.",
            zh: "{count} 条在等。看一眼不等于取走：信号要等会话取走才离开这个队列。",
        },
        Msg::SettingsInterfaceTitle => Phrase {
            en: "the interface takes its type from your browser",
            zh: "界面的字体取自你的浏览器",
        },
        Msg::SettingsInterfaceScope => Phrase {
            en: "font family and base size only; everything else on this page is the city's",
            zh: "只管字族与基准字号；本页其余都属于这座城",
        },
        Msg::SettingsInterfaceSource => Phrase {
            en: "no font file ships with this binary and none is fetched from anywhere",
            zh: "本二进制不带任何字体文件，也不从任何地方取",
        },
        Msg::SettingsInterfaceFaces => Phrase {
            en: "Text here is drawn with the two families your browser is set to use - the standard one for prose, the fixed-width one for numbers, addresses and hashes. To change either, open your browser's own font settings; in Chrome and Edge that is Appearance, then Customise fonts. Nothing needs to be set here, and nothing here overrides what you set there.",
            zh: "这里的字用你浏览器设定的两个字族画：正文用标准那个，数字、地址与哈希用等宽那个。要改哪一个，就去开浏览器自己的字体设置；Chrome 与 Edge 在「外观」里的「自定义字体」。这里不需要设任何东西，也不会覆盖你在那边设的。",
        },
        Msg::SettingsInterfaceContent => Phrase {
            en: "A city's own content - a building's name, a document, a ledger payload - can be in any language. Your system already holds a face for it, and this interface does not replace that choice with a guess of its own.",
            zh: "一座城自己的内容——楼名、文档、账本载荷——可以是任何语言。你的系统已经存着能画它的字，本界面不拿自己的猜测去替掉那个选择。",
        },
        Msg::ArchiveHits => Phrase {
            en: "{total} hit(s) for “{needle}” in {shelves} building(s), read from the shelves just now",
            zh: "“{needle}”在 {shelves} 栋楼里命中 {total} 条，刚从书架上读的",
        },
        Msg::ArchiveScope => Phrase {
            en: "two sources, never merged: a search reads the shelves on disk at the moment you ask, and the list below it is folded from history. The same item can appear in both.",
            zh: "两个来源，恒不合并：搜索在你问的那一刻读盘上的书架，下面那份列表折自历史。同一件可以同时出现在两边。",
        },
        Msg::ArchiveSource => Phrase {
            en: "the search reads each building's Archive directory; `filed lately` is folded from the Ledger's asset_archived records, which is why it can say when something was filed and by whom",
            zh: "搜索读的是每栋楼的 Archive 目录；「最近归档」折自 Ledger 的 asset_archived，所以它说得出某件东西何时由谁归的档",
        },
        Msg::ArchiveSearchFor => Phrase {
            en: "search the shelves for",
            zh: "在书架上找",
        },
        Msg::ArchiveWordPlaceholder => Phrase {
            en: "a word the archives may hold",
            zh: "归档里可能有的一个词",
        },
        Msg::ArchiveSearchButton => Phrase {
            en: "search the shelves",
            zh: "搜书架",
        },
        Msg::ArchiveNoSearch => Phrase {
            en: "no search has been run",
            zh: "还没搜过",
        },
        Msg::ArchiveNoSearchWhat => Phrase {
            en: "the shelves are read when you ask and not before, so an empty word searches nothing rather than everything. Type a word above.",
            zh: "书架在你问的时候才读，所以空词搜的是「什么都不搜」而不是「全都搜」。在上面敲一个词。",
        },

        Msg::ArchiveAskingFiled => Phrase {
            en: "asking the record what was filed lately",
            zh: "正在问记录最近归了什么档",
        },
        Msg::ArchiveListWhenArrives => Phrase {
            en: "the list appears when the answer arrives",
            zh: "答案到了列表就出现",
        },
        Msg::ArchiveNothingFiled => Phrase {
            en: "nothing has been filed yet",
            zh: "还没归过档",
        },
        Msg::ArchiveNothingFiledWhat => Phrase {
            en: "a run files an asset when it settles something worth not doing twice. The archive is what the next run is told before it starts, so it fills as work completes rather than as work begins.",
            zh: "会话在定下某件不值得再做一遍的事时归一份档。归档是下一个会话开工前会被告知的东西，所以它随着活干完而变多，而不是随着活开始。",
        },
        Msg::ArchiveFiledLately => Phrase {
            en: "filed lately",
            zh: "最近归档",
        },
        Msg::CostAskingSpent => Phrase {
            en: "asking the city what it has spent",
            zh: "正在问这座城花了多少",
        },
        Msg::CostAskingSpentWhat => Phrase {
            en: "the five cuts appear when the answer arrives; nothing is missing yet",
            zh: "答案到了五个切面就出现；现在还不缺什么",
        },
        Msg::CostNothingSpent => Phrase {
            en: "nothing has been spent yet",
            zh: "还没花过钱",
        },
        Msg::CostUnpricedTitle => Phrase {
            en: "work was done here, and no provider reported a price for it",
            zh: "这里干过活，但没有 provider 报过价",
        },
        Msg::CostWhereMoneyWent => Phrase {
            en: "where the money went",
            zh: "钱去哪了",
        },
        Msg::CostUnpricedScope => Phrase {
            en: "every run in this city since it was raised, in five independent cuts. The rows are what was attributed; the amounts are missing because no call came back with one - a subscription or a local model reports what it used, not what it cost.",
            zh: "这座城建成以来的每一个会话，五个互相独立的切面。行是归因出来的；金额缺失是因为没有一次调用带回价格——订阅制或本地模型报的是用量，不是花费。",
        },
        Msg::CostScope => Phrase {
            en: "every run in this city since it was raised, in five independent cuts of the same total",
            zh: "这座城建成以来的每一个会话，同一个总额的五个互相独立的切面",
        },
        Msg::CostSource => Phrase {
            en: "folded from the Ledger's model_returned records; each cut sums to that same total, and an unattributed remainder stays visible rather than being divided away",
            zh: "折自 Ledger 的 model_returned；每个切面加起来都是那同一个总额，归不出去的余额留在明面上而不是被摊掉",
        },
        Msg::CostNoneBilled => Phrase {
            en: "no model call has been billed in this city",
            zh: "这座城里没有一次模型调用被计过费",
        },
        Msg::CostNoneBilledWhat => Phrase {
            en: "a run that reaches a provider is priced from that provider's own figure, and lands in all five cuts at once. Send work from the bar below and the money appears here.",
            zh: "够到 provider 的会话按那家自己给的数字计价，并一次落进五个切面。从下面那条栏派活，钱就会出现在这里。",
        },
        Msg::CostCutEmpty => Phrase {
            en: "this cut has nothing in it: no call has been attributed to a {dimension} yet",
            zh: "这个切面是空的：还没有调用被归到某个{dimension}上",
        },
        Msg::ProgressNoPlan => Phrase {
            en: "no plan",
            zh: "没有计划",
        },
        Msg::TurnNumber => Phrase {
            en: "turn {n}",
            zh: "第 {n} 轮",
        },
        Msg::TurnTools => Phrase {
            en: "{count} tool call(s)",
            zh: "{count} 次工具调用",
        },
        Msg::TurnNoTools => Phrase {
            en: "no tool was called",
            zh: "没有调用工具",
        },
        Msg::TurnWaiting => Phrase {
            en: "running",
            zh: "在跑",
        },
        Msg::TurnAnswered => Phrase {
            en: "answered",
            zh: "已答",
        },
        Msg::TurnFailed => Phrase {
            en: "failed",
            zh: "失败",
        },
        Msg::TurnTokens => Phrase {
            en: "{input} in, {output} out",
            zh: "{input} 进，{output} 出",
        },
        Msg::TurnStopped => Phrase {
            en: "stopped: {why}",
            zh: "停在：{why}",
        },
        Msg::TurnOutput => Phrase {
            en: "what it said",
            zh: "它说了什么",
        },
        Msg::TurnOutputCut => Phrase {
            en: "{cut} more line(s), in the Ledger at {seq}",
            zh: "还有 {cut} 行，在账本第 {seq} 条",
        },
        Msg::NoteWaiting => Phrase {
            en: "this turn stopped for a person to answer",
            zh: "这一轮停下来等人答",
        },
        Msg::NoteFenced => Phrase {
            en: "checkpoint {oid}",
            zh: "检查点 {oid}",
        },
        Msg::NoteArrived => Phrase {
            en: "{from} said something",
            zh: "{from} 说了一句",
        },
        Msg::NoteDiscarded => Phrase {
            en: "{count} file(s) went away, each with its way back",
            zh: "{count} 个文件没了，每个都带着回去的路",
        },
        Msg::ChangedFiles => Phrase {
            en: "{count} file(s) this session changed",
            zh: "这次会话动过的文件 {count}",
        },
        Msg::ChangedNothing => Phrase {
            en: "nothing on disk has moved since this session opened",
            zh: "这次会话开工以来，盘上没有东西动过",
        },
        Msg::ChangedAdded => Phrase {
            en: "new",
            zh: "新增",
        },
        Msg::ChangedModified => Phrase {
            en: "changed",
            zh: "改动",
        },
        Msg::ChangedDeleted => Phrase {
            en: "gone",
            zh: "删除",
        },
        Msg::ChangedRenamed => Phrase {
            en: "moved",
            zh: "改名",
        },
        Msg::ChangedBinary => Phrase {
            en: "not text",
            zh: "非文本",
        },
        Msg::LiveEveryEvent => Phrase {
            en: "every event, one line each",
            zh: "每一条事件，各一行",
        },
        Msg::PalettePlaceholder => Phrase {
            en: "go to a page, a building or a session",
            zh: "去某一页、某栋楼或某个会话",
        },
        Msg::PaletteNothing => Phrase {
            en: "nothing matches",
            zh: "没有匹配的",
        },
        Msg::PaletteNothingWhat => Phrase {
            en: "part of a page name, a building name or a session id is enough",
            zh: "页名、楼名或会话 id 的一部分就够了",
        },
        Msg::PaletteKindPage => Phrase {
            en: "page",
            zh: "页",
        },
        Msg::PaletteKindBuilding => Phrase {
            en: "building",
            zh: "楼",
        },
        Msg::PaletteKindSession => Phrase {
            en: "session",
            zh: "会话",
        },
        Msg::KeysTitle => Phrase {
            en: "keys",
            zh: "快捷键",
        },
        Msg::KeysScope => Phrase {
            en: "a letter on its own works outside a text box; the two with a modifier work everywhere",
            zh: "单个字母在输入框外才算；带修饰键的那两个到处都算",
        },
        Msg::KeysPalette => Phrase {
            en: "go to a page, a building or a session",
            zh: "去某一页、某栋楼或某个会话",
        },
        Msg::KeysCompose => Phrase {
            en: "start work here, or send what is already written",
            zh: "在这里开工，或送出已经写好的",
        },
        Msg::KeysDismiss => Phrase {
            en: "close what is open",
            zh: "关掉打开的东西",
        },
        Msg::KeysGo => Phrase {
            en: "then o, c, s, a or l: overview, city, sessions, approvals, ledger",
            zh: "再按 o、c、s、a 或 l：总览、城市、会话、审批、账本",
        },
        Msg::KeysShow => Phrase {
            en: "this list",
            zh: "这张表",
        },
        Msg::AlertCannot => Phrase {
            en: "cannot {action} on {subject}",
            zh: "不能对 {subject} 执行 {action}",
        },
        Msg::AlertNoRecovery => Phrase {
            en: "no way out was recorded with this refusal",
            zh: "这条拒绝没有附带出路",
        },
        Msg::AlertAwaitingApproval => Phrase {
            en: "awaiting approval",
            zh: "等待审批",
        },
        Msg::AlertRunFrozen => Phrase {
            en: "run frozen",
            zh: "会话已冻结",
        },
        Msg::AlertProviderTrouble => Phrase {
            en: "provider trouble",
            zh: "provider 出问题",
        },
        Msg::AlertRefused => Phrase {
            en: "refused",
            zh: "被拒绝",
        },
        Msg::AlertSomethingWaiting => Phrase {
            en: "something is waiting for you",
            zh: "有东西在等你",
        },
        Msg::AlertRunStopped => Phrase {
            en: "a run stopped and will not start itself again",
            zh: "一个会话停了，它自己不会再启动",
        },
        Msg::AlertProviderNotAnswering => Phrase {
            en: "the provider is not answering as it should",
            zh: "provider 的答复不对劲",
        },
        Msg::StatusNoCity => Phrase {
            en: "no city",
            zh: "未命名的城",
        },
        Msg::StatusProvider => Phrase {
            en: "provider {state}",
            zh: "provider 状态：{state}",
        },
        Msg::StatusNothingSpent => Phrase {
            en: "nothing spent since this page connected",
            zh: "本页连上之后没花过钱",
        },
        Msg::StatusAwaitingYou => Phrase {
            en: "{count} awaiting you",
            zh: "{count} 件等你处理",
        },
        Msg::StatusAwaitingAndUnreadable => Phrase {
            en: "{count} awaiting you - and {blind} this page cannot read",
            zh: "{count} 件等你处理——另有 {blind} 件本页读不了",
        },
        Msg::StatusUsedNoPrice => Phrase {
            en: "{used} used - no price reported",
            zh: "用了 {used}——对方没报价",
        },
        Msg::StatusSpent => Phrase {
            en: "{spent} spent - {used}",
            zh: "花了 {spent}——{used}",
        },
        Msg::StatusSpentSomeUnpriced => Phrase {
            en: "{spent} spent - {used} - {calls} call(s) unpriced",
            zh: "花了 {spent}——{used}——另有 {calls} 次调用没报价",
        },
        Msg::RouteNoSuchPage => Phrase {
            en: "this build has no page at {named}",
            zh: "本构建没有 {named} 这一页",
        },
        Msg::RouteNoSuchPageRecovery => Phrase {
            en: "the pages this build has are in the list on the left; the address bar shows the one you are on",
            zh: "本构建有的页面都在左栏那份列表里；地址栏显示的是你正在看的那一页",
        },
        Msg::SettingsAttachIt => Phrase {
            en: "attach it",
            zh: "接上它",
        },
        Msg::SettingsNeedsName => Phrase {
            en: "give this provider a name you will recognise later",
            zh: "给这个 provider 起个你以后认得出的名字",
        },
        Msg::SettingsNeedsUrl => Phrase {
            en: "paste the base URL from the provider's documentation",
            zh: "把 provider 文档里的 base URL 贴进来",
        },
        Msg::SettingsUrlNotSafe => Phrase {
            en: "use https, or http only for a server on this machine",
            zh: "用 https；http 只允许指向本机的服务",
        },
        Msg::SettingsNeedsDialect => Phrase {
            en: "say which wire this provider speaks",
            zh: "说明这个 provider 说哪种线格式",
        },
        Msg::SettingsOnThisMachine => Phrase {
            en: "on this machine",
            zh: "在本机",
        },
        Msg::SettingsOffThisMachine => Phrase {
            en: "off this machine",
            zh: "在机器外",
        },
        Msg::SettingsWithCredential => Phrase {
            en: "with an enrolled credential",
            zh: "已登记凭证",
        },
        Msg::SettingsNoCredential => Phrase {
            en: "with no credential",
            zh: "没有凭证",
        },
        Msg::SettingsMainConsequence => Phrase {
            en: "without this, a dispatch is refused",
            zh: "不选它，派活会被拒",
        },
        Msg::SettingsDigestConsequence => Phrase {
            en: "without this, long documents are read whole by the main model",
            zh: "不选它，长文档会由主模型整篇读",
        },
        Msg::SettingsUnknownConsequence => Phrase {
            en: "the effect of leaving this unset is not recorded",
            zh: "不选它的后果没有记录在案",
        },
        Msg::SettingsStoredAs => Phrase {
            en: "stored as {reference}; the key itself is now only in the vault",
            zh: "已存为 {reference}；密钥本身现在只在保险库里",
        },
        Msg::SettingsUseThisModel => Phrase {
            en: "use this model for that job",
            zh: "这个活就用这个模型",
        },
        Msg::SettingsPickProvider => Phrase {
            en: "pick which provider serves it",
            zh: "先选哪个 provider 提供它",
        },
        Msg::SettingsPickModel => Phrase {
            en: "pick a model",
            zh: "选一个模型",
        },
        Msg::SettingsPickJob => Phrase {
            en: "say what this model is for",
            zh: "说明这个模型派什么用",
        },
        Msg::SettingsModelNotServed => Phrase {
            en: "that endpoint does not list this model",
            zh: "那个端点没有列出这个模型",
        },
        Msg::SettingsAsking => Phrase {
            en: "asking the server what is attached",
            zh: "正在问服务端接着什么",
        },
        Msg::SettingsAskingWhat => Phrase {
            en: "providers, the model chosen for each job, and what is missing before this city can be dispatched to",
            zh: "provider、每个活选定的模型，以及这座城能被派活之前还缺什么",
        },
        Msg::SettingsDispatchable => Phrase {
            en: "this city can be dispatched to",
            zh: "这座城可以派活了",
        },
        Msg::SettingsNotDispatchable => Phrase {
            en: "no model answers for main, so a dispatch is refused",
            zh: "没有模型答 main 这个活，所以派活会被拒",
        },
        Msg::SettingsScope => Phrase {
            en: "every provider attached to this city, and which of its models answers for each job",
            zh: "接到这座城上的每个 provider，以及各自哪个模型答哪个活",
        },
        Msg::SettingsSource => Phrase {
            en: "the city's own endpoint book, re-read whenever a provider is attached or a model is chosen",
            zh: "这座城自己那本端点账，每次接上 provider 或选定模型就重读一次",
        },
        Msg::SettingsNoProvider => Phrase {
            en: "no provider is attached",
            zh: "还没接上任何 provider",
        },
        Msg::SettingsNoProviderWhat => Phrase {
            en: "a run needs a model to answer for `main`. Attach one below with a base URL and a key, or sign in with a subscription. Nothing here is bundled and nothing is proxied - the endpoint is yours.",
            zh: "会话需要一个模型来答 `main`。在下面用 base URL 加密钥接一个，或者用订阅登录。这里不捆绑任何东西，也不代理任何东西——端点是你自己的。",
        },
        Msg::SettingsAttachProvider => Phrase {
            en: "Attach a provider",
            zh: "接一个 provider",
        },
        Msg::SettingsCallIt => Phrase {
            en: "call it",
            zh: "叫它",
        },
        Msg::SettingsNamePlaceholder => Phrase {
            en: "a name you will recognise",
            zh: "一个你认得出的名字",
        },
        Msg::SettingsBaseUrl => Phrase {
            en: "base URL",
            zh: "base URL（接口地址）",
        },
        Msg::SettingsUrlHint => Phrase {
            en: "https anywhere; http only to this machine",
            zh: "https 任处；http 只到本机",
        },
        Msg::SettingsWhichWire => Phrase {
            en: "which wire does it speak",
            zh: "它说哪种线格式",
        },
        Msg::SettingsKey => Phrase {
            en: "key",
            zh: "密钥",
        },
        Msg::SettingsKeyPlaceholder => Phrase {
            en: "the provider's key, or empty for a local server",
            zh: "provider 的密钥；本机服务留空",
        },
        Msg::SettingsKeyHint => Phrase {
            en: "it leaves this page for the machine's own credential vault and comes back as a reference; it is never put in a frame and never shown again",
            zh: "它离开本页直奔这台机器自己的凭证保险库，回来的是一条引用；它恒不进入任何帧，也不再显示",
        },
        Msg::SettingsPutKeyInVault => Phrase {
            en: "put the key in the vault",
            zh: "把密钥放进保险库",
        },
        Msg::SettingsAttachThisProvider => Phrase {
            en: "attach this provider",
            zh: "接上这个 provider",
        },
        Msg::DropAction => Phrase {
            en: "read what was dropped",
            zh: "读懂被拖进来的东西",
        },
        Msg::DropRecovery => Phrase {
            en: "drop it on a building or a room; work is aimed at a place, and the button below still starts it",
            zh: "把它放到一栋楼或一个房间上；活是派往一个地方的，而下面那个按钮仍然是启动它的地方",
        },
        Msg::DropNotAPlace => Phrase {
            en: "a run is something that happened at an address, not a place work can be put",
            zh: "一个会话是在某个地址上发生过的事，不是一个可以放活进去的地方",
        },
        Msg::DropUnreadable => Phrase {
            en: "this build could not read anything in what was dropped",
            zh: "这一版读不出被拖进来的东西里的任何内容",
        },
        Msg::DropHere => Phrase {
            en: "drop work here",
            zh: "把活拖到这里",
        },
        Msg::BuildingReachTab => Phrase {
            en: "what it may reach",
            zh: "它够得到什么",
        },
        Msg::BuildingShell => Phrase {
            en: "offer the shell arm",
            zh: "开放 shell 那条臂",
        },
        Msg::BuildingFuel => Phrase {
            en: "instruction budget for one sandboxed call",
            zh: "一次沙箱调用的指令预算",
        },
        Msg::BuildingMounts => Phrase {
            en: "extra readable paths, one per line",
            zh: "额外可读路径，每行一条",
        },
        Msg::BuildingServers => Phrase {
            en: "external servers, one per line as `label url` or `label ! command`",
            zh: "外部服务器，每行一条，写成 `label url` 或 `label ! 命令`",
        },
        Msg::BuildingServersHint => Phrase {
            en: "an empty list is a building that reaches none, which is not the same as saying nothing",
            zh: "空表意味着这栋楼一个都不够到，与「什么都没说」不是一回事",
        },
        Msg::BuildingSaveReach => Phrase {
            en: "save what this building may reach",
            zh: "保存这栋楼够得到的东西",
        },
        Msg::SettingsAskWhatItServes => Phrase {
            en: "ask what it serves",
            zh: "问它供应什么",
        },
        Msg::SettingsServes => Phrase {
            en: "what it serves; tick the ones this city may use",
            zh: "它供应的模型；勾选本城可以用的那些",
        },
        Msg::SettingsAdmitAll => Phrase {
            en: "nothing ticked admits everything it serves",
            zh: "一个都不勾即全部准入",
        },
        Msg::SettingsSignIn => Phrase {
            en: "Sign in with a subscription",
            zh: "用订阅登录",
        },
        Msg::SettingsProvider => Phrase {
            en: "provider",
            zh: "provider（供应方）",
        },
        Msg::SettingsStartLogin => Phrase {
            en: "start the login",
            zh: "开始登录",
        },
        Msg::SettingsNoLoginWaiting => Phrase {
            en: "no login is waiting",
            zh: "没有登录在等着",
        },
        Msg::SettingsOpenApproveePaste => Phrase {
            en: "open this, approve, and paste the code the provider shows you",
            zh: "打开它、批准，然后把 provider 显示的那串码贴回来",
        },
        Msg::SettingsCodeLabel => Phrase {
            en: "the code the provider showed you",
            zh: "provider 显示给你的那串码",
        },
        Msg::SettingsPasteHere => Phrase {
            en: "paste it here",
            zh: "贴在这里",
        },
        Msg::SettingsFinishLogin => Phrase {
            en: "finish the login",
            zh: "完成登录",
        },
        Msg::SettingsChooseModelHeading => Phrase {
            en: "choose a model for a job",
            zh: "给一个活选模型",
        },
        Msg::SettingsWhichProvider => Phrase {
            en: "which provider",
            zh: "哪个 provider",
        },
        Msg::SettingsWhichModel => Phrase {
            en: "which model",
            zh: "哪个模型",
        },
        Msg::SettingsForWhichJob => Phrase {
            en: "for which job",
            zh: "派什么用",
        },
        Msg::SettingsWhatFor => Phrase {
            en: "what for",
            zh: "用来做什么",
        },
        Msg::SettingsPointJobAtModel => Phrase {
            en: "point this job at that model",
            zh: "把这个活指向那个模型",
        },
        Msg::SettingsWhatEachModelIsFor => Phrase {
            en: "what each model is for",
            zh: "每个模型派什么用",
        },
        Msg::SettingsWhatIsAttached => Phrase {
            en: "what is attached now",
            zh: "现在接着什么",
        },
        Msg::SettingsModelCount => Phrase {
            en: "{count} model(s)",
            zh: "{count} 个模型",
        },
        Msg::SettingsReadItAgain => Phrase {
            en: "read it again",
            zh: "再读一次",
        },
        Msg::BoardTitle => Phrase {
            en: "the plan",
            zh: "计划",
        },
        Msg::BoardScope => Phrase {
            en: "only leaves count: a branch's work is its children, and counting both would count the same effort twice",
            zh: "只有叶子算数：一根枝的活就是它的子节点，两边都算等于把同一份力气数两遍",
        },
        Msg::BoardSource => Phrase {
            en: "drawn from this building's Roadmap.md, which holds every state there is; this page keeps none of its own",
            zh: "画的是这栋楼的 Roadmap.md，状态全在那里，这一页自己不存",
        },
        Msg::BoardReady => Phrase {
            en: "ready",
            zh: "就绪",
        },
        Msg::BoardWaiting => Phrase {
            en: "waiting",
            zh: "等依赖",
        },
        Msg::BoardWorking => Phrase {
            en: "working",
            zh: "在做",
        },
        Msg::BoardBlocked => Phrase {
            en: "stuck",
            zh: "卡住",
        },
        Msg::BoardDone => Phrase {
            en: "done",
            zh: "完成",
        },
        Msg::BoardEmpty => Phrase {
            en: "this building has no plan yet",
            zh: "这栋楼还没有计划",
        },
        Msg::BoardEmptyWhat => Phrase {
            en: "send somebody to write one, and the tree grows as they split the work",
            zh: "派个人去写一份，随后他们每拆一次活，这棵树就长一层",
        },
        Msg::BoardStuck => Phrase {
            en: "what is stuck",
            zh: "卡住的",
        },
        Msg::BoardStuckScope => Phrase {
            en: "one line per cause, never one per symptom",
            zh: "一处原因一行，不是一个症状一行",
        },
        Msg::BoardStuckSource => Phrase {
            en: "from the plan, and from the record that says why it stopped",
            zh: "来自计划表，与说明它为什么停下的那条记录",
        },
        Msg::BoardWaitingBehind => Phrase {
            en: "{n} behind it",
            zh: "{n} 个节点在后面等",
        },
        Msg::BoardWaitsFor => Phrase {
            en: "waits for {nodes}",
            zh: "等 {nodes}",
        },
        Msg::BuildingPlanTab => Phrase {
            en: "plan",
            zh: "计划",
        },
        Msg::SettingsLanguage => Phrase {
            en: "this interface reads in your language",
            zh: "这个界面说你的语言",
        },
        Msg::SettingsLanguageScope => Phrase {
            en: "what this client writes; never what the city wrote",
            zh: "只管这个客户端自己说的话，恒不改城写下的字",
        },
        Msg::SettingsLanguageSource => Phrase {
            en: "your browser's own language, until you choose otherwise here",
            zh: "默认取浏览器自己的语言设置，你在这里选了就以你选的为准",
        },
    }
}

/// The word for one message in one language.
#[must_use]
pub fn say(lang: Lang, msg: Msg) -> &'static str {
    phrase(msg).in_lang(lang)
}

/// Fills the `{named}` slots of a phrase.
///
/// A sentence with a number in it cannot be a `&'static str`, and it
/// cannot be a `format!` either: the pattern is chosen at runtime by the
/// language, and `format!` takes a literal. So the slots are named and
/// filled here - named rather than positional, because the two languages
/// put them in different orders and a positional slot would silently
/// swap two numbers.
///
/// A slot the caller did not fill is left as it is written, which shows
/// on screen. That is deliberate: a visible `{runs}` is a defect
/// somebody reports, and a silently emptied sentence is one nobody does.
#[must_use]
pub fn fill(pattern: &str, slots: &[(&str, &str)]) -> String {
    let mut out = pattern.to_owned();
    for (name, value) in slots {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// The words on each side of one slot, for a page that draws the slot's
/// own value in its own element.
///
/// [`fill`] answers when the result is one string. This answers when it
/// is not: the composer underlines the inferred word and leaves the
/// words around it plain, which needs the two halves separately.
///
/// Splitting rather than assuming the value comes last is what keeps
/// this working in both languages and in the next one: `as {mode}` puts
/// it at the end and a language that says it first would silently
/// reverse the sentence under a rule that hard-coded the order.
///
/// A pattern without the slot yields the whole phrase and an empty tail,
/// so a missing slot renders as a sentence with a word absent rather
/// than as an empty screen.
#[must_use]
pub fn around(pattern: &'static str, slot: &str) -> (&'static str, &'static str) {
    match pattern.split_once(&format!("{{{slot}}}")) {
        Some(halves) => halves,
        None => (pattern, ""),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::{Lang, Msg, phrase, say};

    /// The slot names a pattern uses, in the order they appear.
    fn slots_of(pattern: &str) -> Vec<&str> {
        let mut found = Vec::new();
        let mut rest = pattern;
        while let Some(open) = rest.find('{') {
            let Some(after) = rest.get(open.saturating_add(1)..) else {
                break;
            };
            match after.find('}') {
                None => break,
                Some(close) => {
                    if let Some(name) = after.get(..close) {
                        found.push(name);
                    }
                    rest = after.get(close.saturating_add(1)..).unwrap_or_default();
                }
            }
        }
        found.sort_unstable();
        found
    }

    /// Every message this build knows, so the assertions below are
    /// exhaustive by construction rather than by a list somebody keeps
    /// up to date. A variant added without a line here fails to compile.
    fn every_message() -> Vec<Msg> {
        let all = [
            Msg::CityNoBuildings,
            Msg::NavTheRecord,
            Msg::NavCity,
            Msg::NavCost,
            Msg::NavSettings,
            Msg::PhaseRunning,
            Msg::PhaseWaiting,
            Msg::PhaseFrozen,
            Msg::PhaseCancelled,
            Msg::PhaseHalted,
            Msg::NavSessions,
            Msg::NavWaiting,
            Msg::ComposerTitle,
            Msg::ComposerScope,
            Msg::ComposerExample,
            Msg::ComposerSendTo,
            Msg::ComposerAs,
            Msg::ComposerThink,
            Msg::ComposerKeys,
            Msg::ComposerSource,
            Msg::ComposerRoomFor,
            Msg::ComposerModeFor,
            Msg::ComposerEffortFor,
            Msg::SessionsScope,
            Msg::SessionsSource,
            Msg::SessionsEnded,
            Msg::SessionsEndedScope,
            Msg::SessionsEndedSource,
            Msg::SessionsNothingYet,
            Msg::SessionsNothingWhat,
            Msg::SessionsTurnCount,
            Msg::SessionsUnpriced,
            Msg::SessionsCityScope,
            Msg::SessionAllSessions,
            Msg::SessionTurnOrdinal,
            Msg::SessionSpentIs,
            Msg::SessionAtGate,
            Msg::SessionNoGate,
            Msg::SessionContextUnknown,
            Msg::SessionContextScope,
            Msg::SessionHandoffAt,
            Msg::SessionHandoffNone,
            Msg::SessionHandoffJust,
            Msg::SessionTabTurns,
            Msg::SessionTabChanges,
            Msg::SessionTabCost,
            Msg::SessionTabDocs,
            Msg::SessionTabPrompt,
            Msg::PromptTitle,
            Msg::PromptScope,
            Msg::PromptSource,
            Msg::PromptAtTurn,
            Msg::PromptBytes,
            Msg::PromptNone,
            Msg::PromptNoneWhat,
            Msg::PromptSkillsTitle,
            Msg::PromptSkillsScope,
            Msg::PromptSkillsSource,
            Msg::PromptNoSkills,
            Msg::PromptNoSkillsWhat,
            Msg::PromptSkillFirst,
            Msg::PromptSkillSame,
            Msg::PromptSkillChanged,
            Msg::SessionScope,
            Msg::SessionSource,
            Msg::SessionUnknown,
            Msg::SessionUnknownWhat,
            Msg::WaitingScope,
            Msg::WaitingSource,
            Msg::WaitingNothing,
            Msg::WaitingNothingWhat,
            Msg::WaitingFrozenHeading,
            Msg::RecordScope,
            Msg::RecordLensLedger,
            Msg::RecordLensArchive,
            Msg::RecordLensBin,
            Msg::FirstNoModelTitle,
            Msg::FirstNoModelScope,
            Msg::FirstNoModelStatus,
            Msg::FirstNoModelWhat,
            Msg::FirstNoModelWay,
            Msg::FirstNoModelSubscription,
            Msg::FirstNoModelSource,
            Msg::FirstNoBuildingTitle,
            Msg::FirstNoBuildingScope,
            Msg::FirstNoBuildingStatus,
            Msg::FirstNoBuildingWhat,
            Msg::FirstNoBuildingWay,
            Msg::FirstNoBuildingSource,
            Msg::FirstDispatchTitle,
            Msg::FirstDispatchScope,
            Msg::FirstDispatchKeys,
            Msg::FirstDispatchSource,
            Msg::PursuitTitle,
            Msg::PursuitScope,
            Msg::PursuitSource,
            Msg::PursuitEmpty,
            Msg::PursuitEmptyWhat,
            Msg::PursuitSet,
            Msg::PursuitPause,
            Msg::PursuitResume,
            Msg::PursuitClear,
            Msg::PursuitGoalLabel,
            Msg::AutonomyTitle,
            Msg::AutonomyScope,
            Msg::AutonomySource,
            Msg::AutonomyOwner,
            Msg::AutonomyDeferred,
            Msg::CityRunning,
            Msg::CityRunningIdle,
            Msg::CityStopped,
            Msg::CityUnwell,
            Msg::CountRunning,
            Msg::CountWaiting,
            Msg::CountBuildings,
            Msg::EffortInherited,
            Msg::EffortLow,
            Msg::EffortMedium,
            Msg::EffortHigh,
            Msg::EffortXHigh,
            Msg::EffortMax,
            Msg::DispatchSend,
            Msg::HaltCity,
            Msg::ReleaseCity,
            Msg::CancelLastRun,
            Msg::VitalsRecords,
            Msg::VitalsSignals,
            Msg::VitalsDiscards,
            Msg::VitalsAsking,
            Msg::ProviderUnknown,
            Msg::ProviderHealthy,
            Msg::ProviderDegraded,
            Msg::ProviderLost,
            Msg::AskingWhatItHolds,
            Msg::LineToolCalled,
            Msg::LineToolResult,
            Msg::LineModelCalled,
            Msg::LineModelReturned,
            Msg::LineSteered,
            Msg::LineGateDenied,
            Msg::LineApprovalRequested,
            Msg::LineRunFrozen,
            Msg::LiveNothingSinceConnected,
            Msg::LiveOneSession,
            Msg::LiveEveryRun,
            Msg::LiveScope,
            Msg::LiveSource,
            Msg::LiveEverything,
            Msg::LiveRunId,
            Msg::LiveFollowEnd,
            Msg::LiveDropped,
            Msg::LiveNoRunYet,
            Msg::LiveNothingSince,
            Msg::LiveNoRunYetWhat,
            Msg::LiveNothingSinceWhat,
            Msg::LivePickASession,
            Msg::LiveSteerPlaceholder,
            Msg::LiveSteerSend,
            Msg::LiveForkFrom,
            Msg::LiveNothingToBranch,
            Msg::LiveInterventionNote,
            Msg::CityScope,
            Msg::CityTowerNote,
            Msg::CitySource,
            Msg::CityStanding,
            Msg::CityStageLabel,
            Msg::CityBuildingNamePlaceholder,
            Msg::CityRaiseBuilding,
            Msg::CityNoBuildingsWhat,
            Msg::ReadIt,
            Msg::CityMoveLeft,
            Msg::CityMoveRight,
            Msg::CityMoveUp,
            Msg::CityMoveDown,
            Msg::CityFit,
            Msg::CityReadWhat,
            Msg::CityWhatShouldHappen,
            Msg::CityWhatCountsAsDone,
            Msg::CitySendWorkHere,
            Msg::CityClearSelection,
            Msg::BuildingAsking,
            Msg::BuildingAskingWhat,
            Msg::BuildingTitle,
            Msg::BuildingScope,
            Msg::BuildingSource,
            Msg::BuildingStartHere,
            Msg::BuildingNoRooms,
            Msg::BuildingUnreadableRow,
            Msg::BuildingArchiveTab,
            Msg::BuildingAskingRoom,
            Msg::BuildingAskingRoomWhat,
            Msg::BuildingRoomEmpty,
            Msg::BuildingRoomEmptyWhat,
            Msg::BuildingSignalFrom,
            Msg::BuildingNothingFiled,
            Msg::BuildingNothingFiledWhat,
            Msg::BuildingTruncated,
            Msg::BuildingNoDocument,
            Msg::ApprovalNothingWaiting,
            Msg::ApprovalTitle,
            Msg::ApprovalScope,
            Msg::ApprovalSource,
            Msg::ApprovalNoneEscalated,
            Msg::ApprovalNoneEscalatedWhat,
            Msg::ApprovalTainted,
            Msg::BinRestoreCheckpoint,
            Msg::BinRestoreStored,
            Msg::BinRebuild,
            Msg::BinNoDescription,
            Msg::BinAsking,
            Msg::BinAskingWhat,
            Msg::BinNothingDiscarded,
            Msg::BinTitle,
            Msg::BinScope,
            Msg::BinSource,
            Msg::BinNoneYet,
            Msg::BinNoneYetWhat,
            Msg::BinAlreadyRestored,
            Msg::BinRollback,
            Msg::BinRollbackNote,
            Msg::LedgerNothingSaid,
            Msg::LedgerTitle,
            Msg::LedgerScope,
            Msg::LedgerSource,
            Msg::LedgerWhoActed,
            Msg::LedgerAnyPartOfName,
            Msg::LedgerKindOfEvent,
            Msg::LedgerEveryKind,
            Msg::LedgerNoMatch,
            Msg::LedgerNothingArrived,
            Msg::LedgerFilterNote,
            Msg::LedgerFirstLineNote,
            Msg::LedgerNewestMatching,
            Msg::LedgerTakeThisPage,
            Msg::ArchiveTitle,
            Msg::ArchiveHits,
            Msg::ArchiveScope,
            Msg::LedgerNewer,
            Msg::LedgerOlder,
            Msg::LedgerSkipped,
            Msg::BuildingWaitingCount,
            Msg::SettingsInterfaceTitle,
            Msg::SettingsInterfaceScope,
            Msg::SettingsInterfaceSource,
            Msg::SettingsInterfaceFaces,
            Msg::SettingsInterfaceContent,
            Msg::ArchiveSource,
            Msg::ArchiveSearchFor,
            Msg::ArchiveWordPlaceholder,
            Msg::ArchiveSearchButton,
            Msg::ArchiveNoSearch,
            Msg::ArchiveNoSearchWhat,
            Msg::ArchiveAskingFiled,
            Msg::ArchiveListWhenArrives,
            Msg::ArchiveNothingFiled,
            Msg::ArchiveNothingFiledWhat,
            Msg::ArchiveFiledLately,
            Msg::CostAskingSpent,
            Msg::CostAskingSpentWhat,
            Msg::CostNothingSpent,
            Msg::CostUnpricedTitle,
            Msg::CostWhereMoneyWent,
            Msg::CostUnpricedScope,
            Msg::CostScope,
            Msg::CostSource,
            Msg::CostNoneBilled,
            Msg::CostNoneBilledWhat,
            Msg::CostCutEmpty,
            Msg::ProgressNoPlan,
            Msg::TurnNumber,
            Msg::TurnTools,
            Msg::TurnNoTools,
            Msg::TurnWaiting,
            Msg::TurnAnswered,
            Msg::TurnFailed,
            Msg::TurnTokens,
            Msg::TurnStopped,
            Msg::TurnOutput,
            Msg::TurnOutputCut,
            Msg::NoteWaiting,
            Msg::NoteFenced,
            Msg::NoteArrived,
            Msg::NoteDiscarded,
            Msg::ChangedFiles,
            Msg::ChangedNothing,
            Msg::ChangedAdded,
            Msg::ChangedModified,
            Msg::ChangedDeleted,
            Msg::ChangedRenamed,
            Msg::ChangedBinary,
            Msg::LiveEveryEvent,
            Msg::PalettePlaceholder,
            Msg::PaletteNothing,
            Msg::PaletteNothingWhat,
            Msg::PaletteKindPage,
            Msg::PaletteKindBuilding,
            Msg::PaletteKindSession,
            Msg::KeysTitle,
            Msg::KeysScope,
            Msg::KeysPalette,
            Msg::KeysCompose,
            Msg::KeysDismiss,
            Msg::KeysGo,
            Msg::KeysShow,
            Msg::AlertCannot,
            Msg::AlertNoRecovery,
            Msg::AlertAwaitingApproval,
            Msg::AlertRunFrozen,
            Msg::AlertProviderTrouble,
            Msg::AlertRefused,
            Msg::AlertSomethingWaiting,
            Msg::AlertRunStopped,
            Msg::AlertProviderNotAnswering,
            Msg::StatusNoCity,
            Msg::StatusProvider,
            Msg::StatusNothingSpent,
            Msg::StatusAwaitingYou,
            Msg::StatusAwaitingAndUnreadable,
            Msg::StatusUsedNoPrice,
            Msg::StatusSpent,
            Msg::StatusSpentSomeUnpriced,
            Msg::RouteNoSuchPage,
            Msg::RouteNoSuchPageRecovery,
            Msg::SettingsAttachIt,
            Msg::SettingsNeedsName,
            Msg::SettingsNeedsUrl,
            Msg::SettingsUrlNotSafe,
            Msg::SettingsNeedsDialect,
            Msg::SettingsOnThisMachine,
            Msg::SettingsOffThisMachine,
            Msg::SettingsWithCredential,
            Msg::SettingsNoCredential,
            Msg::SettingsMainConsequence,
            Msg::SettingsDigestConsequence,
            Msg::SettingsUnknownConsequence,
            Msg::SettingsStoredAs,
            Msg::SettingsUseThisModel,
            Msg::SettingsPickProvider,
            Msg::SettingsPickModel,
            Msg::SettingsPickJob,
            Msg::SettingsModelNotServed,
            Msg::SettingsAsking,
            Msg::SettingsAskingWhat,
            Msg::SettingsDispatchable,
            Msg::SettingsNotDispatchable,
            Msg::SettingsScope,
            Msg::SettingsSource,
            Msg::SettingsNoProvider,
            Msg::SettingsNoProviderWhat,
            Msg::SettingsAttachProvider,
            Msg::SettingsCallIt,
            Msg::SettingsNamePlaceholder,
            Msg::SettingsBaseUrl,
            Msg::SettingsUrlHint,
            Msg::SettingsWhichWire,
            Msg::SettingsKey,
            Msg::SettingsKeyPlaceholder,
            Msg::SettingsKeyHint,
            Msg::SettingsPutKeyInVault,
            Msg::SettingsAttachThisProvider,
            Msg::DropAction,
            Msg::DropRecovery,
            Msg::DropNotAPlace,
            Msg::DropUnreadable,
            Msg::DropHere,
            Msg::BuildingReachTab,
            Msg::BuildingShell,
            Msg::BuildingFuel,
            Msg::BuildingMounts,
            Msg::BuildingServers,
            Msg::BuildingServersHint,
            Msg::BuildingSaveReach,
            Msg::SettingsAskWhatItServes,
            Msg::SettingsServes,
            Msg::SettingsAdmitAll,
            Msg::SettingsSignIn,
            Msg::SettingsProvider,
            Msg::SettingsStartLogin,
            Msg::SettingsNoLoginWaiting,
            Msg::SettingsOpenApproveePaste,
            Msg::SettingsCodeLabel,
            Msg::SettingsPasteHere,
            Msg::SettingsFinishLogin,
            Msg::SettingsChooseModelHeading,
            Msg::SettingsWhichProvider,
            Msg::SettingsWhichModel,
            Msg::SettingsForWhichJob,
            Msg::SettingsWhatFor,
            Msg::SettingsPointJobAtModel,
            Msg::SettingsWhatEachModelIsFor,
            Msg::SettingsWhatIsAttached,
            Msg::SettingsModelCount,
            Msg::SettingsReadItAgain,
            Msg::BoardTitle,
            Msg::BoardScope,
            Msg::BoardSource,
            Msg::BoardReady,
            Msg::BoardWaiting,
            Msg::BoardWorking,
            Msg::BoardBlocked,
            Msg::BoardDone,
            Msg::BoardEmpty,
            Msg::BoardEmptyWhat,
            Msg::BoardStuck,
            Msg::BoardStuckScope,
            Msg::BoardStuckSource,
            Msg::BoardWaitingBehind,
            Msg::BoardWaitsFor,
            Msg::BuildingPlanTab,
            Msg::SettingsLanguage,
            Msg::SettingsLanguageScope,
            Msg::SettingsLanguageSource,
        ];
        for msg in all {
            match msg {
                Msg::CityNoBuildings
                | Msg::NavTheRecord
                | Msg::NavCity
                | Msg::NavCost
                | Msg::NavSettings
                | Msg::PhaseRunning
                | Msg::PhaseWaiting
                | Msg::PhaseFrozen
                | Msg::PhaseCancelled
                | Msg::PhaseHalted
                | Msg::NavSessions
                | Msg::NavWaiting
                | Msg::ComposerTitle
                | Msg::ComposerScope
                | Msg::ComposerExample
                | Msg::ComposerSendTo
                | Msg::ComposerAs
                | Msg::ComposerThink
                | Msg::ComposerKeys
                | Msg::ComposerSource
                | Msg::ComposerRoomFor
                | Msg::ComposerModeFor
                | Msg::ComposerEffortFor
                | Msg::SessionsScope
                | Msg::SessionsSource
                | Msg::SessionsEnded
                | Msg::SessionsEndedScope
                | Msg::SessionsEndedSource
                | Msg::SessionsNothingYet
                | Msg::SessionsNothingWhat
                | Msg::SessionsTurnCount
                | Msg::SessionsUnpriced
                | Msg::SessionsCityScope
                | Msg::SessionAllSessions
                | Msg::SessionTurnOrdinal
                | Msg::SessionSpentIs
                | Msg::SessionAtGate
                | Msg::SessionNoGate
                | Msg::SessionContextUnknown
                | Msg::SessionContextScope
                | Msg::SessionHandoffAt
                | Msg::SessionHandoffNone
                | Msg::SessionHandoffJust
                | Msg::SessionTabTurns
                | Msg::SessionTabChanges
                | Msg::SessionTabCost
                | Msg::SessionTabDocs
                | Msg::SessionTabPrompt
                | Msg::PromptTitle
                | Msg::PromptScope
                | Msg::PromptSource
                | Msg::PromptAtTurn
                | Msg::PromptBytes
                | Msg::PromptNone
                | Msg::PromptNoneWhat
                | Msg::PromptSkillsTitle
                | Msg::PromptSkillsScope
                | Msg::PromptSkillsSource
                | Msg::PromptNoSkills
                | Msg::PromptNoSkillsWhat
                | Msg::PromptSkillFirst
                | Msg::PromptSkillSame
                | Msg::PromptSkillChanged
                | Msg::SessionScope
                | Msg::SessionSource
                | Msg::SessionUnknown
                | Msg::SessionUnknownWhat
                | Msg::WaitingScope
                | Msg::WaitingSource
                | Msg::WaitingNothing
                | Msg::WaitingNothingWhat
                | Msg::WaitingFrozenHeading
                | Msg::RecordScope
                | Msg::RecordLensLedger
                | Msg::RecordLensArchive
                | Msg::RecordLensBin
                | Msg::FirstNoModelTitle
                | Msg::FirstNoModelScope
                | Msg::FirstNoModelStatus
                | Msg::FirstNoModelWhat
                | Msg::FirstNoModelWay
                | Msg::FirstNoModelSubscription
                | Msg::FirstNoModelSource
                | Msg::FirstNoBuildingTitle
                | Msg::FirstNoBuildingScope
                | Msg::FirstNoBuildingStatus
                | Msg::FirstNoBuildingWhat
                | Msg::FirstNoBuildingWay
                | Msg::FirstNoBuildingSource
                | Msg::FirstDispatchTitle
                | Msg::FirstDispatchScope
                | Msg::FirstDispatchKeys
                | Msg::FirstDispatchSource
                | Msg::PursuitTitle
                | Msg::PursuitScope
                | Msg::PursuitSource
                | Msg::PursuitEmpty
                | Msg::PursuitEmptyWhat
                | Msg::PursuitSet
                | Msg::PursuitPause
                | Msg::PursuitResume
                | Msg::PursuitClear
                | Msg::PursuitGoalLabel
                | Msg::AutonomyTitle
                | Msg::AutonomyScope
                | Msg::AutonomySource
                | Msg::AutonomyOwner
                | Msg::AutonomyDeferred
                | Msg::CityRunning
                | Msg::CityRunningIdle
                | Msg::CityStopped
                | Msg::CityUnwell
                | Msg::CountRunning
                | Msg::CountWaiting
                | Msg::CountBuildings
                | Msg::EffortInherited
                | Msg::EffortLow
                | Msg::EffortMedium
                | Msg::EffortHigh
                | Msg::EffortXHigh
                | Msg::EffortMax
                | Msg::DispatchSend
                | Msg::HaltCity
                | Msg::ReleaseCity
                | Msg::CancelLastRun
                | Msg::VitalsRecords
                | Msg::VitalsSignals
                | Msg::VitalsDiscards
                | Msg::VitalsAsking
                | Msg::ProviderUnknown
                | Msg::ProviderHealthy
                | Msg::ProviderDegraded
                | Msg::ProviderLost
                | Msg::AskingWhatItHolds
                | Msg::LineToolCalled
                | Msg::LineToolResult
                | Msg::LineModelCalled
                | Msg::LineModelReturned
                | Msg::LineSteered
                | Msg::LineGateDenied
                | Msg::LineApprovalRequested
                | Msg::LineRunFrozen
                | Msg::LiveNothingSinceConnected
                | Msg::LiveOneSession
                | Msg::LiveEveryRun
                | Msg::LiveScope
                | Msg::LiveSource
                | Msg::LiveEverything
                | Msg::LiveRunId
                | Msg::LiveFollowEnd
                | Msg::LiveDropped
                | Msg::LiveNoRunYet
                | Msg::LiveNothingSince
                | Msg::LiveNoRunYetWhat
                | Msg::LiveNothingSinceWhat
                | Msg::LivePickASession
                | Msg::LiveSteerPlaceholder
                | Msg::LiveSteerSend
                | Msg::LiveForkFrom
                | Msg::LiveNothingToBranch
                | Msg::LiveInterventionNote
                | Msg::CityScope
                | Msg::CityTowerNote
                | Msg::CitySource
                | Msg::CityStanding
                | Msg::CityStageLabel
                | Msg::CityBuildingNamePlaceholder
                | Msg::CityRaiseBuilding
                | Msg::CityNoBuildingsWhat
                | Msg::ReadIt
                | Msg::CityMoveLeft
                | Msg::CityMoveRight
                | Msg::CityMoveUp
                | Msg::CityMoveDown
                | Msg::CityFit
                | Msg::CityReadWhat
                | Msg::CityWhatShouldHappen
                | Msg::CityWhatCountsAsDone
                | Msg::CitySendWorkHere
                | Msg::CityClearSelection
                | Msg::BuildingAsking
                | Msg::BuildingAskingWhat
                | Msg::BuildingTitle
                | Msg::BuildingScope
                | Msg::BuildingSource
                | Msg::BuildingStartHere
                | Msg::BuildingNoRooms
                | Msg::BuildingUnreadableRow
                | Msg::BuildingArchiveTab
                | Msg::BuildingAskingRoom
                | Msg::BuildingAskingRoomWhat
                | Msg::BuildingRoomEmpty
                | Msg::BuildingRoomEmptyWhat
                | Msg::BuildingSignalFrom
                | Msg::BuildingNothingFiled
                | Msg::BuildingNothingFiledWhat
                | Msg::BuildingTruncated
                | Msg::BuildingNoDocument
                | Msg::ApprovalNothingWaiting
                | Msg::ApprovalTitle
                | Msg::ApprovalScope
                | Msg::ApprovalSource
                | Msg::ApprovalNoneEscalated
                | Msg::ApprovalNoneEscalatedWhat
                | Msg::ApprovalTainted
                | Msg::BinRestoreCheckpoint
                | Msg::BinRestoreStored
                | Msg::BinRebuild
                | Msg::BinNoDescription
                | Msg::BinAsking
                | Msg::BinAskingWhat
                | Msg::BinNothingDiscarded
                | Msg::BinTitle
                | Msg::BinScope
                | Msg::BinSource
                | Msg::BinNoneYet
                | Msg::BinNoneYetWhat
                | Msg::BinAlreadyRestored
                | Msg::BinRollback
                | Msg::BinRollbackNote
                | Msg::LedgerNothingSaid
                | Msg::LedgerTitle
                | Msg::LedgerScope
                | Msg::LedgerSource
                | Msg::LedgerWhoActed
                | Msg::LedgerAnyPartOfName
                | Msg::LedgerKindOfEvent
                | Msg::LedgerEveryKind
                | Msg::LedgerNoMatch
                | Msg::LedgerNothingArrived
                | Msg::LedgerFilterNote
                | Msg::LedgerFirstLineNote
                | Msg::LedgerNewestMatching
                | Msg::LedgerTakeThisPage
                | Msg::ArchiveTitle
                | Msg::ArchiveHits
                | Msg::ArchiveScope
                | Msg::LedgerNewer
                | Msg::LedgerOlder
                | Msg::LedgerSkipped
                | Msg::BuildingWaitingCount
                | Msg::SettingsInterfaceTitle
                | Msg::SettingsInterfaceScope
                | Msg::SettingsInterfaceSource
                | Msg::SettingsInterfaceFaces
                | Msg::SettingsInterfaceContent
                | Msg::ArchiveSource
                | Msg::ArchiveSearchFor
                | Msg::ArchiveWordPlaceholder
                | Msg::ArchiveSearchButton
                | Msg::ArchiveNoSearch
                | Msg::ArchiveNoSearchWhat
                | Msg::ArchiveAskingFiled
                | Msg::ArchiveListWhenArrives
                | Msg::ArchiveNothingFiled
                | Msg::ArchiveNothingFiledWhat
                | Msg::ArchiveFiledLately
                | Msg::CostAskingSpent
                | Msg::CostAskingSpentWhat
                | Msg::CostNothingSpent
                | Msg::CostUnpricedTitle
                | Msg::CostWhereMoneyWent
                | Msg::CostUnpricedScope
                | Msg::CostScope
                | Msg::CostSource
                | Msg::CostNoneBilled
                | Msg::CostNoneBilledWhat
                | Msg::CostCutEmpty
                | Msg::ProgressNoPlan
                | Msg::TurnNumber
                | Msg::TurnTools
                | Msg::TurnNoTools
                | Msg::TurnWaiting
                | Msg::TurnAnswered
                | Msg::TurnFailed
                | Msg::TurnTokens
                | Msg::TurnStopped
                | Msg::TurnOutput
                | Msg::TurnOutputCut
                | Msg::NoteWaiting
                | Msg::NoteFenced
                | Msg::NoteArrived
                | Msg::NoteDiscarded
                | Msg::ChangedFiles
                | Msg::ChangedNothing
                | Msg::ChangedAdded
                | Msg::ChangedModified
                | Msg::ChangedDeleted
                | Msg::ChangedRenamed
                | Msg::ChangedBinary
                | Msg::LiveEveryEvent
                | Msg::PalettePlaceholder
                | Msg::PaletteNothing
                | Msg::PaletteNothingWhat
                | Msg::PaletteKindPage
                | Msg::PaletteKindBuilding
                | Msg::PaletteKindSession
                | Msg::KeysTitle
                | Msg::KeysScope
                | Msg::KeysPalette
                | Msg::KeysCompose
                | Msg::KeysDismiss
                | Msg::KeysGo
                | Msg::KeysShow
                | Msg::AlertCannot
                | Msg::AlertNoRecovery
                | Msg::AlertAwaitingApproval
                | Msg::AlertRunFrozen
                | Msg::AlertProviderTrouble
                | Msg::AlertRefused
                | Msg::AlertSomethingWaiting
                | Msg::AlertRunStopped
                | Msg::AlertProviderNotAnswering
                | Msg::StatusNoCity
                | Msg::StatusProvider
                | Msg::StatusNothingSpent
                | Msg::StatusAwaitingYou
                | Msg::StatusAwaitingAndUnreadable
                | Msg::StatusUsedNoPrice
                | Msg::StatusSpent
                | Msg::StatusSpentSomeUnpriced
                | Msg::RouteNoSuchPage
                | Msg::RouteNoSuchPageRecovery
                | Msg::SettingsAttachIt
                | Msg::SettingsNeedsName
                | Msg::SettingsNeedsUrl
                | Msg::SettingsUrlNotSafe
                | Msg::SettingsNeedsDialect
                | Msg::SettingsOnThisMachine
                | Msg::SettingsOffThisMachine
                | Msg::SettingsWithCredential
                | Msg::SettingsNoCredential
                | Msg::SettingsMainConsequence
                | Msg::SettingsDigestConsequence
                | Msg::SettingsUnknownConsequence
                | Msg::SettingsStoredAs
                | Msg::SettingsUseThisModel
                | Msg::SettingsPickProvider
                | Msg::SettingsPickModel
                | Msg::SettingsPickJob
                | Msg::SettingsModelNotServed
                | Msg::SettingsAsking
                | Msg::SettingsAskingWhat
                | Msg::SettingsDispatchable
                | Msg::SettingsNotDispatchable
                | Msg::SettingsScope
                | Msg::SettingsSource
                | Msg::SettingsNoProvider
                | Msg::SettingsNoProviderWhat
                | Msg::SettingsAttachProvider
                | Msg::SettingsCallIt
                | Msg::SettingsNamePlaceholder
                | Msg::SettingsBaseUrl
                | Msg::SettingsUrlHint
                | Msg::SettingsWhichWire
                | Msg::SettingsKey
                | Msg::SettingsKeyPlaceholder
                | Msg::SettingsKeyHint
                | Msg::SettingsPutKeyInVault
                | Msg::SettingsAttachThisProvider
                | Msg::DropAction
                | Msg::DropRecovery
                | Msg::DropNotAPlace
                | Msg::DropUnreadable
                | Msg::DropHere
                | Msg::BuildingReachTab
                | Msg::BuildingShell
                | Msg::BuildingFuel
                | Msg::BuildingMounts
                | Msg::BuildingServers
                | Msg::BuildingServersHint
                | Msg::BuildingSaveReach
                | Msg::SettingsAskWhatItServes
                | Msg::SettingsServes
                | Msg::SettingsAdmitAll
                | Msg::SettingsSignIn
                | Msg::SettingsProvider
                | Msg::SettingsStartLogin
                | Msg::SettingsNoLoginWaiting
                | Msg::SettingsOpenApproveePaste
                | Msg::SettingsCodeLabel
                | Msg::SettingsPasteHere
                | Msg::SettingsFinishLogin
                | Msg::SettingsChooseModelHeading
                | Msg::SettingsWhichProvider
                | Msg::SettingsWhichModel
                | Msg::SettingsForWhichJob
                | Msg::SettingsWhatFor
                | Msg::SettingsPointJobAtModel
                | Msg::SettingsWhatEachModelIsFor
                | Msg::SettingsWhatIsAttached
                | Msg::SettingsModelCount
                | Msg::SettingsReadItAgain
                | Msg::BoardTitle
                | Msg::BoardScope
                | Msg::BoardSource
                | Msg::BoardReady
                | Msg::BoardWaiting
                | Msg::BoardWorking
                | Msg::BoardBlocked
                | Msg::BoardDone
                | Msg::BoardEmpty
                | Msg::BoardEmptyWhat
                | Msg::BoardStuck
                | Msg::BoardStuckScope
                | Msg::BoardStuckSource
                | Msg::BoardWaitingBehind
                | Msg::BoardWaitsFor
                | Msg::BuildingPlanTab
                | Msg::SettingsLanguage
                | Msg::SettingsLanguageScope
                | Msg::SettingsLanguageSource => {}
            }
        }
        all.to_vec()
    }

    #[test]
    fn nothing_is_left_untranslated_or_left_as_english_by_accident() {
        for msg in every_message() {
            let said = phrase(msg);
            assert!(!said.en.trim().is_empty(), "{msg:?} has no English");
            assert!(!said.zh.trim().is_empty(), "{msg:?} has no Chinese");
            // The defect this catches is a phrase copied through
            // untranslated. A Han character is not the test for it: a
            // cell that reads `3 / 8` is right in both languages, and
            // demanding a word there would put one in that nobody needs.
            assert_ne!(
                said.zh, said.en,
                "{msg:?} was copied rather than translated"
            );
        }
    }

    #[test]
    fn a_browser_asking_in_chinese_is_answered_in_chinese() {
        for tag in ["zh", "zh-CN", "zh-Hans-CN", "ZH-TW"] {
            assert_eq!(Lang::of(tag), Lang::Zh, "{tag}");
        }
        for tag in ["en", "en-GB", "de", "", "zzh"] {
            assert_eq!(Lang::of(tag), Lang::En, "{tag}");
        }
    }

    /// Two languages of one sentence must take the same values. A slot
    /// present in one and absent in the other is a number that appears
    /// for one reader and vanishes for the other.
    #[test]
    fn both_languages_of_a_sentence_ask_for_the_same_values() {
        for msg in every_message() {
            let said = phrase(msg);
            assert_eq!(
                slots_of(said.en),
                slots_of(said.zh),
                "{msg:?} fills different slots in each language"
            );
        }
    }

    #[test]
    fn a_slot_nobody_filled_stays_visible_rather_than_disappearing() {
        assert_eq!(super::fill("{a} of {b}", &[("a", "3")]), "3 of {b}");
        assert_eq!(
            super::fill("{a} of {b}", &[("a", "3"), ("b", "4")]),
            "3 of 4"
        );
    }

    #[test]
    fn a_language_names_itself_in_itself() {
        assert_eq!(Lang::Zh.endonym(), "中文");
        assert_eq!(Lang::En.endonym(), "English");
        assert_eq!(say(Lang::Zh, Msg::DispatchSend), "派出去");
        assert_eq!(say(Lang::En, Msg::DispatchSend), "send it");
    }

    /// Every module that draws something a person reads.
    ///
    /// Listed rather than walked, because this crate compiles to wasm and
    /// a test that read the directory would be testing the machine it ran
    /// on. A view added without a line here is a view whose English can
    /// escape, which is the failure this table exists to make loud.
    const VIEWS: [(&str, &str); 25] = [
        ("alert.rs", include_str!("alert.rs")),
        ("board.rs", include_str!("board.rs")),
        ("app.rs", include_str!("app.rs")),
        ("approval.rs", include_str!("approval.rs")),
        ("archive_search.rs", include_str!("archive_search.rs")),
        ("building_view.rs", include_str!("building_view.rs")),
        ("city_view.rs", include_str!("city_view.rs")),
        ("dashboard.rs", include_str!("dashboard.rs")),
        ("drop.rs", include_str!("drop.rs")),
        ("ledger_view.rs", include_str!("ledger_view.rs")),
        ("live.rs", include_str!("live.rs")),
        ("turn.rs", include_str!("turn.rs")),
        ("palette.rs", include_str!("palette.rs")),
        ("panel.rs", include_str!("panel.rs")),
        ("phase.rs", include_str!("phase.rs")),
        ("record.rs", include_str!("record.rs")),
        ("session.rs", include_str!("session.rs")),
        ("sessions.rs", include_str!("sessions.rs")),
        ("waiting.rs", include_str!("waiting.rs")),
        ("progress.rs", include_str!("progress.rs")),
        ("prompt.rs", include_str!("prompt.rs")),
        ("pursuit.rs", include_str!("pursuit.rs")),
        ("reach.rs", include_str!("reach.rs")),
        ("settings.rs", include_str!("settings.rs")),
        ("vitals.rs", include_str!("vitals.rs")),
    ];

    /// The part of a module that draws, which is everything above its own
    /// test module.
    ///
    /// Load-bearing in both directions: a phrase named only by a test is
    /// not a phrase any reader sees, and a sentence quoted by a test to
    /// assert that the page says it is evidence rather than a second
    /// authority for the wording.
    fn drawn(body: &str) -> &str {
        match body.find("#[cfg(test)]") {
            Some(at) => body.get(..at).unwrap_or(body),
            None => body,
        }
    }

    /// A phrase nothing renders is an English literal standing in its
    /// place.
    ///
    /// The exhaustive `match` above proves every variant is *translated*.
    /// It cannot prove any of them reaches a screen, and that gap is not
    /// hypothetical: nineteen of these were dead at once, seventeen of
    /// them because the same English had been written into a view as a
    /// literal, so the Chinese page rendered whole English paragraphs
    /// while every assertion in this crate stayed green.
    #[test]
    fn every_phrase_is_said_by_some_view() {
        let painted: String = VIEWS.iter().map(|&(_, body)| drawn(body)).collect();
        let mut unsaid = Vec::new();
        for msg in every_message() {
            let named = format!("{msg:?}");
            if !painted.contains(&format!("Msg::{named}")) {
                unsaid.push(named);
            }
        }
        assert!(
            unsaid.is_empty(),
            "phrases no view renders, so something else is saying them: {unsaid:?}"
        );
    }

    /// The English of a phrase may not also be written as a literal.
    ///
    /// The sharper half of the rule above. A view can call `word(...)` in
    /// one arm and inline the same sentence in another, which is how the
    /// city page ended up with an English paragraph while the phrase it
    /// belonged to was in use elsewhere.
    #[test]
    fn no_view_writes_out_a_sentence_the_phrase_table_already_holds() {
        let mut copied = Vec::new();
        for msg in every_message() {
            let english = say(Lang::En, msg);
            // Long enough that a collision is the same sentence rather
            // than a shared word: "page", "newer" and "cost" are phrases
            // too, and they turn up inside identifiers and comments.
            if english.len() < 40 {
                continue;
            }
            for &(name, body) in &VIEWS {
                if drawn(body).contains(english) {
                    copied.push(format!("{name}: {msg:?}"));
                }
            }
        }
        assert!(
            copied.is_empty(),
            "a view spells out a sentence the table already says: {copied:?}"
        );
    }
}
