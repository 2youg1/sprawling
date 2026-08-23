// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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
    NavHappeningNow,
    NavTheRecord,
    NavSetup,
    NavOverview,
    NavCity,
    NavLive,
    NavApprovals,
    NavLedger,
    NavArchive,
    NavRecycleBin,
    NavCost,
    NavSettings,
    // The control surface: the one place work is started.
    DispatchRoom,
    DispatchRoomHint,
    DispatchCallIt,
    DispatchCallItHint,
    DispatchTask,
    DispatchTaskHint,
    DispatchDoneWhen,
    DispatchDoneWhenHint,
    DispatchMode,
    DispatchSend,
    HaltCity,
    ReleaseCity,
    CancelLastRun,
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
        Msg::NavHappeningNow => Phrase {
            en: "happening now",
            zh: "正在发生",
        },
        Msg::NavTheRecord => Phrase {
            en: "the record",
            zh: "记录",
        },
        Msg::NavSetup => Phrase {
            en: "setup",
            zh: "设置",
        },
        Msg::NavOverview => Phrase {
            en: "overview",
            zh: "总览",
        },
        Msg::NavCity => Phrase {
            en: "city",
            zh: "城市",
        },
        Msg::NavLive => Phrase {
            en: "live",
            zh: "直播",
        },
        Msg::NavApprovals => Phrase {
            en: "approvals",
            zh: "审批",
        },
        Msg::NavLedger => Phrase {
            en: "ledger",
            zh: "账本",
        },
        Msg::NavArchive => Phrase {
            en: "archive",
            zh: "归档",
        },
        Msg::NavRecycleBin => Phrase {
            en: "recycle bin",
            zh: "回收站",
        },
        Msg::NavCost => Phrase {
            en: "cost",
            zh: "成本",
        },
        Msg::NavSettings => Phrase {
            en: "settings",
            zh: "设置",
        },
        Msg::DispatchRoom => Phrase {
            en: "room",
            zh: "去哪",
        },
        Msg::DispatchRoomHint => Phrase {
            en: "lab",
            zh: "楼名，或 楼/房间",
        },
        Msg::DispatchCallIt => Phrase {
            en: "call it",
            zh: "叫什么",
        },
        Msg::DispatchCallItHint => Phrase {
            en: "give it a name",
            zh: "给这次会话取个名字",
        },
        Msg::DispatchTask => Phrase {
            en: "task",
            zh: "干什么",
        },
        Msg::DispatchTaskHint => Phrase {
            en: "what to produce, in one line",
            zh: "一句话说清要产出什么",
        },
        Msg::DispatchDoneWhen => Phrase {
            en: "done when",
            zh: "何时算完",
        },
        Msg::DispatchDoneWhenHint => Phrase {
            en: "what counts as done, and when to stop",
            zh: "什么算做完，什么时候停",
        },
        Msg::DispatchMode => Phrase {
            en: "mode",
            zh: "模式",
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
            en: "cancel the last run",
            zh: "取消最近一次 Run",
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

    /// Every message this build knows, so the assertions below are
    /// exhaustive by construction rather than by a list somebody keeps
    /// up to date. A variant added without a line here fails to compile.
    fn every_message() -> Vec<Msg> {
        let all = [
            Msg::NavHappeningNow,
            Msg::NavTheRecord,
            Msg::NavSetup,
            Msg::NavOverview,
            Msg::NavCity,
            Msg::NavLive,
            Msg::NavApprovals,
            Msg::NavLedger,
            Msg::NavArchive,
            Msg::NavRecycleBin,
            Msg::NavCost,
            Msg::NavSettings,
            Msg::DispatchRoom,
            Msg::DispatchRoomHint,
            Msg::DispatchCallIt,
            Msg::DispatchCallItHint,
            Msg::DispatchTask,
            Msg::DispatchTaskHint,
            Msg::DispatchDoneWhen,
            Msg::DispatchDoneWhenHint,
            Msg::DispatchMode,
            Msg::DispatchSend,
            Msg::HaltCity,
            Msg::ReleaseCity,
            Msg::CancelLastRun,
            Msg::SettingsLanguage,
            Msg::SettingsLanguageScope,
            Msg::SettingsLanguageSource,
        ];
        for msg in all {
            match msg {
                Msg::NavHappeningNow
                | Msg::NavTheRecord
                | Msg::NavSetup
                | Msg::NavOverview
                | Msg::NavCity
                | Msg::NavLive
                | Msg::NavApprovals
                | Msg::NavLedger
                | Msg::NavArchive
                | Msg::NavRecycleBin
                | Msg::NavCost
                | Msg::NavSettings
                | Msg::DispatchRoom
                | Msg::DispatchRoomHint
                | Msg::DispatchCallIt
                | Msg::DispatchCallItHint
                | Msg::DispatchTask
                | Msg::DispatchTaskHint
                | Msg::DispatchDoneWhen
                | Msg::DispatchDoneWhenHint
                | Msg::DispatchMode
                | Msg::DispatchSend
                | Msg::HaltCity
                | Msg::ReleaseCity
                | Msg::CancelLastRun
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
            assert!(
                said.zh
                    .chars()
                    .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "{msg:?} was copied rather than translated: {}",
                said.zh
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

    #[test]
    fn a_language_names_itself_in_itself() {
        assert_eq!(Lang::Zh.endonym(), "中文");
        assert_eq!(Lang::En.endonym(), "English");
        assert_eq!(say(Lang::Zh, Msg::DispatchSend), "派出去");
        assert_eq!(say(Lang::En, Msg::DispatchSend), "send it");
    }
}
