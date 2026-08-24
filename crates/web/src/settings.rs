// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The settings page: where a person turns a URL and a key into a model
//! the city can call.
//!
//! Three steps, in the order a person can actually perform them. The key
//! goes first, to `POST /enroll`, and comes back as a reference — it is
//! the one thing on this page that never becomes a Command, because a
//! Command carrying a credential has no byte form. Then the endpoint is
//! attached, which is when the server asks it what it serves. Then a
//! model is pointed at a tag.
//!
//! Every judgement here is a pure function over what the person typed
//! and what the server answered. The component reads those answers and
//! holds nothing: a page that decided anything would be a second place
//! where "is this registration complete" is defined.

use crate::lang::{Msg, fill, say};
use channels::{ChosenSummary, ClientFrame, DialectKind, EndpointSummary, EndpointsAnswer};
use channels::{IdemKey, LoginStep, ModelTag, ProviderName, Query, RunId, Seq, WireCommand};
use dioxus::prelude::*;

use crate::socket::Enrolment;

/// What the person has filled in. Strings because that is what a form
/// yields; the judgement about whether they are usable is [`ready`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachForm {
    pub name: String,
    pub base_url: String,
    pub dialect: Option<DialectKind>,
    /// The reference returned by the enrolment route, not the key. A
    /// local server that asks for nothing leaves it empty.
    pub secret: Option<String>,
    /// The models the person ticked after asking what this base URL
    /// serves. Empty admits everything, which is what somebody who
    /// never asked meant.
    pub admit: Vec<String>,
}

/// Why a form cannot be submitted yet, or that it can. Exhaustive, so
/// the page always has a sentence and never a disabled button with no
/// explanation beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachReadiness {
    Ready,
    NeedsName,
    NeedsUrl,
    /// A URL that is neither https nor loopback http. Refused here for
    /// the same reason the adapter refuses it: a credential must not
    /// cross a plaintext link to somewhere else.
    UrlNotSafe,
    NeedsDialect,
}

impl AttachReadiness {
    /// The sentence shown beside the form.
    #[must_use]
    pub fn sentence(&self) -> Msg {
        match self {
            AttachReadiness::Ready => Msg::SettingsAttachIt,
            AttachReadiness::NeedsName => Msg::SettingsNeedsName,
            AttachReadiness::NeedsUrl => Msg::SettingsNeedsUrl,
            AttachReadiness::UrlNotSafe => Msg::SettingsUrlNotSafe,
            AttachReadiness::NeedsDialect => Msg::SettingsNeedsDialect,
        }
    }
}

/// Whether a URL may carry a credential: https anywhere, or plain http
/// only to this machine.
#[must_use]
pub fn url_is_safe(url: &str) -> bool {
    if let Some(rest) = url.strip_prefix("https://") {
        return !rest.is_empty();
    }
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let host = rest
        .split('/')
        .next()
        .unwrap_or_default()
        .rsplit_once(':')
        .map_or(rest.split('/').next().unwrap_or_default(), |(head, _)| head);
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// Whether the attach form can be submitted.
#[must_use]
pub fn ready(form: &AttachForm) -> AttachReadiness {
    if form.name.trim().is_empty() {
        return AttachReadiness::NeedsName;
    }
    if form.base_url.trim().is_empty() {
        return AttachReadiness::NeedsUrl;
    }
    if !url_is_safe(form.base_url.trim()) {
        return AttachReadiness::UrlNotSafe;
    }
    if form.dialect.is_none() {
        return AttachReadiness::NeedsDialect;
    }
    AttachReadiness::Ready
}

/// One row of the attached-endpoint table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointRow {
    pub name: String,
    pub base_url: String,
    pub dialect: DialectKind,
    /// What this endpoint serves, and what a tag may be pointed at.
    pub models: Vec<String>,
    /// Where this endpoint is, and whether a credential is enrolled for
    /// it. Two messages rather than one sentence, because the words are
    /// joined in the reader's own language at the point of drawing.
    pub reach: Msg,
    pub credential: Msg,
}

/// The rows for what is attached.
#[must_use]
pub fn endpoint_rows(answer: &EndpointsAnswer) -> Vec<EndpointRow> {
    answer.endpoints.iter().map(row_of).collect()
}

fn row_of(endpoint: &EndpointSummary) -> EndpointRow {
    let reach = if endpoint.local {
        Msg::SettingsOnThisMachine
    } else {
        Msg::SettingsOffThisMachine
    };
    let credential = if endpoint.has_credential {
        Msg::SettingsWithCredential
    } else {
        Msg::SettingsNoCredential
    };
    EndpointRow {
        name: endpoint.name.clone(),
        base_url: endpoint.base_url.clone(),
        dialect: endpoint.dialect,
        models: endpoint.models.clone(),
        reach,
        credential,
    }
}

/// One tag and what answers for it, including the tags nothing answers
/// for yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRow {
    pub tag: ModelTag,
    /// `None` means no model is chosen; the page shows what that costs
    /// rather than leaving the row blank.
    pub chosen: Option<ChosenSummary>,
    pub consequence: Msg,
}

/// Every tag this build knows, in the order the page offers them.
///
/// Tags with nothing chosen are listed too: a settings page that only
/// showed what is configured would hide the thing the person came to
/// fix.
#[must_use]
pub fn tag_rows(answer: &EndpointsAnswer) -> Vec<TagRow> {
    ModelTag::ALL
        .into_iter()
        .map(|tag| TagRow {
            tag,
            chosen: answer
                .chosen
                .iter()
                .find(|choice| choice.tag == tag)
                .cloned(),
            consequence: consequence_of(tag),
        })
        .collect()
}

/// What not choosing a model for this tag means. Stated as the effect on
/// the person's work, because "main is unset" tells them nothing.
fn consequence_of(tag: ModelTag) -> Msg {
    match tag {
        ModelTag::Main => Msg::SettingsMainConsequence,
        ModelTag::Digest => Msg::SettingsDigestConsequence,
        // A tag added upstream without a sentence here still appears,
        // saying only that its effect is unrecorded.
        _ => Msg::SettingsUnknownConsequence,
    }
}

/// The command a filled form asks for, or `None` while it is not ready.
///
/// Returning `None` rather than a half-built command is what keeps
/// [`ready`] the only statement of what a complete form is.
/// One login step, ready to send.
///
/// Pure and separate from the button so both steps are decided in one
/// place: the key is derived from the step itself, so pressing "start"
/// twice begins one login while "finish" carries a key of its own.
#[must_use]
pub fn login_command(provider: &str, step: LoginStep) -> Option<WireCommand> {
    let provider = ProviderName::parse(provider.trim()).ok()?;
    let material = match &step {
        LoginStep::Begin => format!("login-begin:{}", provider.as_str()),
        LoginStep::Code { code } => format!("login-code:{}:{code}", provider.as_str()),
    };
    Some(WireCommand::Login {
        provider,
        step,
        idem: IdemKey::derive(&RunId::CITY, Seq::FIRST, material.as_bytes()),
    })
}

#[must_use]
pub fn attach_command(form: &AttachForm) -> Option<WireCommand> {
    if ready(form) != AttachReadiness::Ready {
        return None;
    }
    let name = ProviderName::parse(form.name.trim()).ok()?;
    let base_url = form.base_url.trim().to_owned();
    Some(WireCommand::AttachEndpoint {
        name,
        // The idempotency key is derived from what the person entered,
        // so pressing the button twice attaches once.
        idem: IdemKey::derive(&RunId::CITY, Seq::FIRST, base_url.as_bytes()),
        base_url,
        dialect: form.dialect?,
        secret: form.secret.clone(),
        auth_header: None,
        admit: form.admit.clone(),
    })
}

/// The command that asks a base URL what it serves, without attaching
/// anything.
///
/// It needs the same filled form an attachment does, because a probe
/// that guessed the dialect would be asking a different question from
/// the one the attachment will ask.
#[must_use]
pub fn probe_command(form: &AttachForm) -> Option<WireCommand> {
    if ready(form) != AttachReadiness::Ready {
        return None;
    }
    let name = ProviderName::parse(form.name.trim()).ok()?;
    let base_url = form.base_url.trim().to_owned();
    Some(WireCommand::ProbeEndpoint {
        name,
        // Distinct from the attach key on the same URL: asking and
        // registering are two acts, and one must not deduplicate the
        // other away.
        idem: IdemKey::derive(
            &RunId::CITY,
            Seq::FIRST,
            format!("probe:{base_url}").as_bytes(),
        ),
        base_url,
        dialect: form.dialect?,
        secret: form.secret.clone(),
        auth_header: None,
    })
}

/// What an enrolment answer means to the person watching, and what it
/// leaves behind. Pure, so the sentence and the reference are decided in
/// one place rather than inside a browser callback.
#[must_use]
pub fn enrolment_note(lang: crate::lang::Lang, answer: &Enrolment) -> (Option<String>, String) {
    match answer {
        Enrolment::Stored { reference } => (
            Some(reference.clone()),
            fill(
                say(lang, Msg::SettingsStoredAs),
                &[("reference", reference)],
            ),
        ),
        Enrolment::Refused { reason } => (None, reason.clone()),
    }
}

/// Whether the city can be dispatched to at all: the one question this
/// page exists to answer, and the one a person checks before closing it.
#[must_use]
pub fn can_dispatch(answer: &EndpointsAnswer) -> bool {
    answer
        .chosen
        .iter()
        .any(|choice| choice.tag == ModelTag::Main)
}

/// What a person picked in the model form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectForm {
    pub endpoint: String,
    pub model: String,
    pub tag: Option<ModelTag>,
}

/// Why a model choice is not yet a command. Same shape as the attach
/// form's readiness, and for the same reason: a disabled button with no
/// sentence beside it is a puzzle.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectReadiness {
    Ready,
    NeedsEndpoint,
    NeedsModel,
    NeedsTag,
    ModelNotServed,
}

impl SelectReadiness {
    #[must_use]
    pub fn sentence(&self) -> Msg {
        match self {
            SelectReadiness::Ready => Msg::SettingsUseThisModel,
            SelectReadiness::NeedsEndpoint => Msg::SettingsPickProvider,
            SelectReadiness::NeedsModel => Msg::SettingsPickModel,
            SelectReadiness::NeedsTag => Msg::SettingsPickJob,
            SelectReadiness::ModelNotServed => Msg::SettingsModelNotServed,
        }
    }
}

/// The one statement of what a complete model choice is.
///
/// The last case is the load-bearing one: a model the endpoint never
/// listed cannot be chosen here either. The server refuses it too, and
/// this is not a second authority — it is the same refusal shown before
/// the person presses anything.
#[must_use]
pub fn select_ready(form: &SelectForm, answer: &EndpointsAnswer) -> SelectReadiness {
    if form.endpoint.trim().is_empty() {
        return SelectReadiness::NeedsEndpoint;
    }
    if form.model.trim().is_empty() {
        return SelectReadiness::NeedsModel;
    }
    if form.tag.is_none() {
        return SelectReadiness::NeedsTag;
    }
    let served = answer
        .endpoints
        .iter()
        .any(|endpoint| endpoint.name == form.endpoint && endpoint.models.contains(&form.model));
    if !served {
        return SelectReadiness::ModelNotServed;
    }
    SelectReadiness::Ready
}

/// What one endpoint serves, and nothing another endpoint serves.
///
/// The model list used to be every model of every attached provider,
/// so a person could pick a name their chosen provider had never heard
/// of and read `that endpoint does not list this model` back. This is
/// not a second authority over what is servable — the server refuses
/// the same pair — it is the same refusal moved to before the click.
pub(crate) fn models_of(answer: &EndpointsAnswer, endpoint: &str) -> Vec<String> {
    answer
        .endpoints
        .iter()
        .filter(|attached| attached.name == endpoint)
        .flat_map(|attached| attached.models.clone())
        .collect()
}

/// The command a filled model form asks for, or `None` while it is not
/// ready.
#[must_use]
pub fn select_command(form: &SelectForm, answer: &EndpointsAnswer) -> Option<WireCommand> {
    if select_ready(form, answer) != SelectReadiness::Ready {
        return None;
    }
    let endpoint = ProviderName::parse(form.endpoint.trim()).ok()?;
    let model = form.model.trim().to_owned();
    Some(WireCommand::SelectModel {
        idem: IdemKey::derive(
            &RunId::CITY,
            Seq::FIRST,
            format!("{}/{model}", form.endpoint).as_bytes(),
        ),
        endpoint,
        model,
        tag: form.tag?,
        // The model's own facts, not numbers invented on this page. The
        // server holds the catalogue; zero means "take what it says".
        context_tokens: 0,
        max_output_tokens: 0,
    })
}

/// How many models an endpoint serves, said.
fn model_count(lang: crate::lang::Lang, count: usize) -> String {
    fill(
        say(lang, Msg::SettingsModelCount),
        &[("count", &count.to_string())],
    )
}

/// The settings page.
///
/// It renders what the server answered and hands every action back to
/// its caller. Deciding nothing is the point: whether a registration is
/// complete is stated once, above, and read here.
#[component]
pub fn Settings(
    answer: Option<EndpointsAnswer>,
    /// Where a person must go to finish a login this session began, from
    /// `app::Snapshot`. Absent means no login is waiting on anybody.
    login_url: Option<String>,
    /// What each base URL answered when it was asked what it serves,
    /// folded from `endpoint_probed`. A person cannot tick a list they
    /// have not seen.
    served: std::collections::BTreeMap<String, Vec<String>>,
    /// Whether the socket is live; see `app::Root`.
    live: Signal<bool>,
    on_frame: EventHandler<ClientFrame>,
) -> Element {
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let mut form = use_signal(AttachForm::default);
    let mut choice = use_signal(SelectForm::default);
    let mut key = use_signal(String::new);
    let mut subscription = use_signal(|| "anthropic".to_owned());
    let mut code = use_signal(String::new);
    let mut enrolment = use_signal(|| None::<String>);
    let asked = use_signal(|| false);
    // This page used to wait to be asked. It has a refresh button, and
    // nothing else in the client ever sent the query - so a person who
    // opened settings saw "asking the server what is attached" until they
    // found the button. A page that needs an answer asks for it.
    use_effect(move || {
        let mut asked = asked;
        if live() && !asked() {
            asked.set(true);
            on_frame.call(ClientFrame::Query(Query::EndpointView));
        }
    });
    let Some(answer) = answer else {
        return rsx! {
            section { class: "settings",
                crate::panel::Empty {
                    status: word(Msg::SettingsAsking).to_owned(),
                    what: word(Msg::SettingsAskingWhat).to_owned(),
                }
            }
        };
    };
    let rows = endpoint_rows(&answer);
    let tags = tag_rows(&answer);
    let dispatchable = can_dispatch(&answer);
    let attached = answer.endpoints.len();
    rsx! {
        section { class: "settings",
            crate::panel::Panel {
                title: if dispatchable {
                        word(Msg::SettingsDispatchable).to_owned()
                    } else {
                        word(Msg::SettingsNotDispatchable).to_owned()
                    },
                figure: (attached > 0).then(|| attached.to_string()),
                scope: word(Msg::SettingsScope).to_owned(),
                source: word(Msg::SettingsSource).to_owned(),
            if attached == 0 {
                crate::panel::Empty {
                    status: word(Msg::SettingsNoProvider).to_owned(),
                    what: word(Msg::SettingsNoProviderWhat).to_owned(),
                }
            }
            h2 { "{word(Msg::SettingsAttachProvider)}" }
            // The credential is not in this form. It goes to the
            // enrolment route first and comes back as a reference,
            // which is the only shape of it a Command can carry.
            form {
                class: "attach",
                // Dioxus 0.7 submits by default; this page never wants a
                // page navigation, so the default is refused explicitly.
                // <https://dioxuslabs.com/learn/0.7/migration/to_07/>
                onsubmit: move |event| {
                    event.prevent_default();
                    let filled = form.read().clone();
                    if let Some(command) = attach_command(&filled) {
                        on_frame.call(ClientFrame::Command(Box::new(command)));
                    }
                },
                div { class: "field",
                    label { r#for: "attach-name", "{word(Msg::SettingsCallIt)}" }
                    input {
                        id: "attach-name",
                        name: "name",
                        placeholder: "{word(Msg::SettingsNamePlaceholder)}",
                        value: "{form.read().name}",
                        oninput: move |event| form.write().name = event.value(),
                    }
                }
                div { class: "field",
                    label { r#for: "attach-url", "{word(Msg::SettingsBaseUrl)}" }
                    input {
                        id: "attach-url",
                        name: "base_url",
                        placeholder: "https://api.provider.example/v1",
                        value: "{form.read().base_url}",
                        oninput: move |event| form.write().base_url = event.value(),
                    }
                    span { class: "hint", "{word(Msg::SettingsUrlHint)}" }
                }
                div { class: "field",
                    label { r#for: "attach-dialect", "{word(Msg::SettingsWhichWire)}" }
                    select {
                        id: "attach-dialect",
                        name: "dialect",
                        onchange: move |event| {
                            form.write().dialect = match event.value().as_str() {
                                "anthropic" => Some(DialectKind::Anthropic),
                                "openai" => Some(DialectKind::OpenAi),
                                _ => None,
                            };
                        },
                        option { value: "", "{word(Msg::SettingsWhichWire)}" }
                        option { value: "anthropic", "anthropic messages" }
                        option { value: "openai", "openai chat completions" }
                    }
                }
                // The key. It is typed here and leaves immediately for
                // the enrolment route; what comes back is the reference,
                // and the key itself is never held by this page, never
                // put in a frame, and never shown again.
                div { class: "field wide",
                    label { r#for: "attach-key", "{word(Msg::SettingsKey)}" }
                    input {
                        id: "attach-key",
                        r#type: "password",
                        name: "key",
                        placeholder: "{word(Msg::SettingsKeyPlaceholder)}",
                        value: "{key}",
                        oninput: move |event| key.set(event.value()),
                    }
                    span { class: "hint",
                        "{word(Msg::SettingsKeyHint)}"
                    }
                }
                button {
                    r#type: "button",
                    disabled: key.read().trim().is_empty()
                        || form.read().name.trim().is_empty(),
                    onclick: move |_| {
                        let realm = form.read().name.trim().to_owned();
                        let typed = key.read().clone();
                        key.set(String::new());
                        let said_in = lang();
                        crate::socket::enrol(&realm, "key", &typed, move |answer| {
                            let (reference, said) = enrolment_note(said_in, &answer);
                            if let Some(reference) = reference {
                                form.write().secret = Some(reference);
                            }
                            enrolment.set(Some(said));
                        });
                    },
                    "{word(Msg::SettingsPutKeyInVault)}"
                }
                if let Some(said) = enrolment.read().clone() {
                    p { class: "enrolment", "{said}" }
                }
                // Asking is free and attaching is not, so the order on
                // screen is the order of the decision: see what a key
                // buys, tick what this city may use, then register it.
                div { class: "field wide",
                    button {
                        r#type: "button",
                        class: "probe",
                        disabled: ready(&form.read()) != AttachReadiness::Ready,
                        onclick: move |_| {
                            let filled = form.read().clone();
                            if let Some(command) = probe_command(&filled) {
                                on_frame.call(ClientFrame::Command(Box::new(command)));
                            }
                        },
                        "{word(Msg::SettingsAskWhatItServes)}"
                    }
                }
                if let Some(models) = served.get(form.read().base_url.trim()) {
                    fieldset { class: "admit",
                        legend { "{word(Msg::SettingsServes)}" }
                        for model in models.clone() {
                            label { class: "admit-row", key: "{model}",
                                input {
                                    r#type: "checkbox",
                                    name: "admit",
                                    value: "{model}",
                                    checked: form.read().admit.iter().any(|held| held == &model),
                                    onchange: {
                                        let model = model.clone();
                                        move |event: Event<FormData>| {
                                            let mut held = form.write();
                                            held.admit.retain(|kept| kept != &model);
                                            if event.checked() {
                                                held.admit.push(model.clone());
                                            }
                                        }
                                    },
                                }
                                span { "{model}" }
                            }
                        }
                        span { class: "hint", "{word(Msg::SettingsAdmitAll)}" }
                    }
                }
                div { class: "field wide submit",
                    button {
                        r#type: "submit",
                        disabled: ready(&form.read()) != AttachReadiness::Ready,
                        "{word(Msg::SettingsAttachThisProvider)}"
                    }
                    if ready(&form.read()) != AttachReadiness::Ready {
                        span { class: "hint blocking", "{word(ready(&form.read()).sentence())}" }
                    }
                }
            }
            h2 { "{word(Msg::SettingsSignIn)}" }
            // Two steps with a person in the middle: the provider shows
            // them a code after they approve, and they bring it back.
            // Nothing here listens on a port, because the provider's own
            // page is where the code is shown.
            div { class: "subscription",
                div { class: "field",
                    label { r#for: "subscription-provider", "{word(Msg::SettingsProvider)}" }
                    select {
                        id: "subscription-provider",
                        name: "subscription_provider",
                        onchange: move |event| subscription.set(event.value()),
                        option { value: "anthropic", "anthropic" }
                        option { value: "openai", "openai" }
                    }
                }
                button {
                    r#type: "button",
                    onclick: move |_| {
                        if let Some(command) = login_command(&subscription.read(), LoginStep::Begin)
                        {
                            on_frame.call(ClientFrame::Command(Box::new(command)));
                        }
                    },
                    "{word(Msg::SettingsStartLogin)}"
                }
                match login_url.clone() {
                    None => rsx! {
                        p { class: "unset", "{word(Msg::SettingsNoLoginWaiting)}" }
                    },
                    Some(url) => rsx! {
                        p { class: "login-step",
                            "{word(Msg::SettingsOpenApproveePaste)}"
                        }
                        a { class: "login-url", href: "{url}", target: "_blank", "{url}" }
                        div { class: "field",
                            label { r#for: "login-code", "{word(Msg::SettingsCodeLabel)}" }
                            input {
                                id: "login-code",
                                name: "code",
                                placeholder: "{word(Msg::SettingsPasteHere)}",
                                value: "{code}",
                                oninput: move |event| code.set(event.value()),
                            }
                        }
                        button {
                            r#type: "button",
                            disabled: code.read().trim().is_empty(),
                            onclick: move |_| {
                                let typed = code.read().trim().to_owned();
                                code.set(String::new());
                                if let Some(command) = login_command(
                                    &subscription.read(),
                                    LoginStep::Code { code: typed },
                                ) {
                                    on_frame.call(ClientFrame::Command(Box::new(command)));
                                }
                            },
                            "{word(Msg::SettingsFinishLogin)}"
                        }
                    },
                }
            }
            h2 { "{word(Msg::SettingsChooseModelHeading)}" }
            form {
                class: "choose",
                onsubmit: {
                    let served = answer.clone();
                    move |event: FormEvent| {
                        event.prevent_default();
                        if let Some(command) = select_command(&choice.read(), &served) {
                            on_frame.call(ClientFrame::Command(Box::new(command)));
                        }
                    }
                },
                div { class: "field",
                    label { r#for: "choose-endpoint", "{word(Msg::SettingsProvider)}" }
                    select {
                        id: "choose-endpoint",
                        name: "endpoint",
                        onchange: move |event| {
                            // The model goes with the provider it came
                            // from: keeping the old name here is keeping
                            // a form that is already refused.
                            let mut picked = choice.write();
                            picked.endpoint = event.value();
                            picked.model = String::new();
                        },
                        option { value: "", "{word(Msg::SettingsWhichProvider)}" }
                        for endpoint in answer.endpoints.clone() {
                            option { key: "{endpoint.name}", value: "{endpoint.name}", "{endpoint.name}" }
                        }
                    }
                }
                div { class: "field",
                    label { r#for: "choose-model", "{word(Msg::SettingsPickModel)}" }
                    select {
                        id: "choose-model",
                        name: "model",
                        onchange: move |event| choice.write().model = event.value(),
                        option { value: "", "{word(Msg::SettingsWhichModel)}" }
                        for model in models_of(&answer, &choice.read().endpoint) {
                            option { key: "{model}", value: "{model}", "{model}" }
                        }
                    }
                }
                div { class: "field",
                    label { r#for: "choose-tag", "{word(Msg::SettingsForWhichJob)}" }
                    select {
                        id: "choose-tag",
                        name: "tag",
                        onchange: move |event| {
                            choice.write().tag = ModelTag::ALL
                                .into_iter()
                                .find(|tag| tag.to_string() == event.value());
                        },
                        option { value: "", "{word(Msg::SettingsWhatFor)}" }
                        for tag in ModelTag::ALL {
                            option { key: "{tag}", value: "{tag}", "{tag}" }
                        }
                    }
                }
                // What is missing is said beside the form, not written on
                // the button. A disabled control whose label is an error
                // message is two things at once and reads as neither.
                div { class: "field",
                    button {
                        r#type: "submit",
                        disabled: select_ready(&choice.read(), &answer) != SelectReadiness::Ready,
                        "{word(Msg::SettingsPointJobAtModel)}"
                    }
                    if select_ready(&choice.read(), &answer) != SelectReadiness::Ready {
                        span { class: "hint blocking",
                            "{word(select_ready(&choice.read(), &answer).sentence())}"
                        }
                    }
                }
            }
            h2 { "{word(Msg::SettingsWhatEachModelIsFor)}" }
            table { class: "tags",
                for row in tags {
                    tr { key: "{row.tag}",
                        td { "{row.tag}" }
                        td {
                            match row.chosen {
                                Some(choice) => rsx! { "{choice.endpoint} / {choice.model}" },
                                None => rsx! { span { class: "unset", "{word(row.consequence)}" } },
                            }
                        }
                    }
                }
            }
            h2 { "{word(Msg::SettingsWhatIsAttached)}" }
            table { class: "endpoints",
                for row in rows {
                    tr { key: "{row.name}",
                        td { "{row.name}" }
                        td { "{row.base_url}" }
                        td { "{word(row.reach)}, {word(row.credential)}" }
                        td {
                            // A provider that serves forty-six models is
                            // a fact; forty-six identifiers run together
                            // is not a reading of it. The count leads,
                            // the list is one disclosure away.
                            details { class: "models",
                                summary { "{model_count(lang(), row.models.len())}" }
                                for model in row.models {
                                    span { key: "{model}", class: "model", "{model}" }
                                }
                            }
                        }
                    }
                }
            }
            button {
                class: "refresh quiet",
                onclick: move |_| on_frame.call(ClientFrame::Query(Query::EndpointView)),
                "{word(Msg::SettingsReadItAgain)}"
            }
            }
            // The language every word this client writes is said in.
            // Above the type panel because it is the setting a person
            // came here for, and beside it because both are about how
            // this interface reads rather than what the city did.
            crate::panel::Panel {
                title: crate::lang::say(lang(), crate::lang::Msg::SettingsLanguage).to_owned(),
                scope: crate::lang::say(lang(), crate::lang::Msg::SettingsLanguageScope).to_owned(),
                source: crate::lang::say(lang(), crate::lang::Msg::SettingsLanguageSource)
                    .to_owned(),
                div { class: "languages",
                    for choice in crate::lang::Lang::ALL {
                        button {
                            key: "{choice}",
                            "aria-current": if lang() == choice { "true" } else { "false" },
                            onclick: move |_| {
                                let mut held = lang;
                                held.set(choice);
                                crate::lang::remember(choice);
                            },
                            "{choice.endonym()}"
                        }
                    }
                }
            }
            // Interface. One setting, and it is not ours to hold.
            crate::panel::Panel {
                title: "the interface takes its type from your browser".to_owned(),
                scope: "font family and base size only; everything else on this page is the city's"
                    .to_owned(),
                source: "no font file ships with this binary and none is fetched from anywhere"
                    .to_owned(),
                p { class: "note",
                    "Text here is drawn with the two families your browser is set to use - the standard one for prose, the fixed-width one for numbers, addresses and hashes. To change either, open your browser's own font settings; in Chrome and Edge that is Appearance, then Customise fonts. Nothing needs to be set here, and nothing here overrides what you set there."
                }
                p { class: "note",
                    "A city's own content - a building's name, a document, a ledger payload - can be in any language. Your system already holds a face for it, and this interface does not replace that choice with a guess of its own."
                }
            }
        }
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
    use super::*;

    fn answer() -> EndpointsAnswer {
        EndpointsAnswer {
            endpoints: vec![EndpointSummary {
                name: "house".to_owned(),
                base_url: "https://api.example.test/v1".to_owned(),
                dialect: DialectKind::OpenAi,
                models: vec!["m-large".to_owned()],
                local: false,
                has_credential: true,
            }],
            chosen: vec![ChosenSummary {
                tag: ModelTag::Main,
                endpoint: "house".to_owned(),
                model: "m-large".to_owned(),
                max_output_tokens: 8_192,
            }],
        }
    }

    #[test]
    fn a_model_the_endpoint_never_listed_cannot_be_chosen_on_the_page_either() {
        let served = answer();
        let mut form = SelectForm::default();
        assert_eq!(select_ready(&form, &served), SelectReadiness::NeedsEndpoint);
        form.endpoint = "house".to_owned();
        assert_eq!(select_ready(&form, &served), SelectReadiness::NeedsModel);
        form.model = "m-invented".to_owned();
        assert_eq!(select_ready(&form, &served), SelectReadiness::NeedsTag);
        form.tag = Some(ModelTag::Main);
        assert_eq!(
            select_ready(&form, &served),
            SelectReadiness::ModelNotServed,
            "the refusal the server would give, given before the person presses anything"
        );
        assert!(select_command(&form, &served).is_none());
        form.model = "m-large".to_owned();
        assert_eq!(select_ready(&form, &served), SelectReadiness::Ready);
        let command = select_command(&form, &served).expect("a ready form is a command");
        match command {
            WireCommand::SelectModel {
                model,
                max_output_tokens,
                ..
            } => {
                assert_eq!(model, "m-large");
                assert_eq!(
                    max_output_tokens, 0,
                    "the ceiling is the model's fact; zero asks the server for it"
                );
            }
            other => panic!("a model choice is a SelectModel, not {other:?}"),
        }
    }

    #[test]
    fn the_model_list_holds_what_the_chosen_provider_serves_and_nothing_else() {
        let mut served = answer();
        served.endpoints.push(EndpointSummary {
            name: "neighbour".to_owned(),
            base_url: "https://other.example.test/v1".to_owned(),
            dialect: DialectKind::Anthropic,
            models: vec!["n-small".to_owned()],
            local: false,
            has_credential: true,
        });
        assert_eq!(models_of(&served, "house"), vec!["m-large".to_owned()]);
        assert_eq!(models_of(&served, "neighbour"), vec!["n-small".to_owned()]);
        assert!(
            models_of(&served, "").is_empty(),
            "with no provider chosen there is nothing to choose from, and the page says so"
        );
    }

    #[test]
    fn a_form_says_which_field_is_missing_rather_than_only_that_it_is_incomplete() {
        let mut form = AttachForm::default();
        assert_eq!(ready(&form), AttachReadiness::NeedsName);
        form.name = "house".to_owned();
        assert_eq!(ready(&form), AttachReadiness::NeedsUrl);
        form.base_url = "https://api.example.test/v1".to_owned();
        assert_eq!(ready(&form), AttachReadiness::NeedsDialect);
        form.dialect = Some(DialectKind::OpenAi);
        assert_eq!(ready(&form), AttachReadiness::Ready);
    }

    #[test]
    fn a_credential_may_not_cross_a_plaintext_link_off_this_machine() {
        assert!(url_is_safe("https://api.example.test/v1"));
        assert!(url_is_safe("http://127.0.0.1:11434/v1"));
        assert!(url_is_safe("http://localhost:1234/v1"));
        assert!(!url_is_safe("http://api.example.test/v1"));
        assert!(!url_is_safe("ftp://api.example.test"));
        assert!(!url_is_safe("api.example.test"));

        let form = AttachForm {
            name: "house".to_owned(),
            base_url: "http://api.example.test/v1".to_owned(),
            dialect: Some(DialectKind::OpenAi),
            secret: Some("secret:house/key".to_owned()),
            admit: Vec::new(),
        };
        assert_eq!(ready(&form), AttachReadiness::UrlNotSafe);
        assert!(
            crate::lang::phrase(ready(&form).sentence())
                .en
                .contains("https")
        );
    }

    #[test]
    fn a_row_says_where_it_reaches_and_whether_it_has_a_credential() {
        let rows = endpoint_rows(&answer());
        assert_eq!(rows[0].reach, Msg::SettingsOffThisMachine);
        assert_eq!(rows[0].credential, Msg::SettingsWithCredential);
        assert_eq!(rows[0].models, vec!["m-large".to_owned()]);
    }

    #[test]
    fn every_tag_is_listed_even_when_nothing_answers_for_it() {
        let rows = tag_rows(&answer());
        assert_eq!(rows.len(), ModelTag::ALL.len());
        let main = rows.iter().find(|row| row.tag == ModelTag::Main).unwrap();
        assert_eq!(main.chosen.as_ref().unwrap().model, "m-large");
        let digest = rows.iter().find(|row| row.tag == ModelTag::Digest).unwrap();
        assert!(digest.chosen.is_none());
        assert!(
            crate::lang::phrase(digest.consequence)
                .en
                .contains("main model"),
            "an unset tag states what it costs, not that it is unset"
        );
    }

    #[test]
    fn an_unready_form_yields_no_command_at_all() {
        let mut form = AttachForm {
            name: "house".to_owned(),
            base_url: "http://api.example.test/v1".to_owned(),
            dialect: Some(DialectKind::OpenAi),
            secret: None,
            admit: Vec::new(),
        };
        assert!(
            attach_command(&form).is_none(),
            "a half-built command would be a second statement of what a complete form is"
        );
        form.base_url = "https://api.example.test/v1".to_owned();
        let Some(WireCommand::AttachEndpoint {
            name,
            base_url,
            dialect,
            secret,
            idem,
            ..
        }) = attach_command(&form)
        else {
            panic!("a ready form asks to attach");
        };
        assert_eq!(name.as_str(), "house");
        assert_eq!(base_url, "https://api.example.test/v1");
        assert_eq!(dialect, DialectKind::OpenAi);
        assert_eq!(secret, None);
        // Pressing twice attaches once: the key is derived from what was
        // entered, not from when the button was pressed.
        let Some(WireCommand::AttachEndpoint { idem: again, .. }) = attach_command(&form) else {
            panic!("a ready form asks to attach");
        };
        assert_eq!(idem, again);
    }

    #[test]
    fn an_enrolment_leaves_a_reference_and_a_sentence_that_says_where_the_key_went() {
        let (reference, said) = enrolment_note(
            crate::lang::Lang::En,
            &Enrolment::Stored {
                reference: "secret:house/key".to_owned(),
            },
        );
        assert_eq!(reference.as_deref(), Some("secret:house/key"));
        assert!(said.contains("only in the vault"));

        // A refusal leaves no reference: a form that kept one would ask
        // the server to redeem a credential nobody stored.
        let (reference, said) = enrolment_note(
            crate::lang::Lang::En,
            &Enrolment::Refused {
                reason: "203.0.113.7 is not on this machine".to_owned(),
            },
        );
        assert_eq!(reference, None);
        assert!(said.contains("not on this machine"));
    }

    #[test]
    fn the_page_answers_whether_this_city_can_be_dispatched_to() {
        assert!(can_dispatch(&answer()));
        let empty = EndpointsAnswer {
            endpoints: Vec::new(),
            chosen: Vec::new(),
        };
        assert!(!can_dispatch(&empty));
        let rows = tag_rows(&empty);
        assert!(
            rows.iter()
                .find(|row| row.tag == ModelTag::Main)
                .is_some_and(|row| crate::lang::phrase(row.consequence).en.contains("refused")),
            "the page states the consequence a person is about to hit"
        );
    }
}
