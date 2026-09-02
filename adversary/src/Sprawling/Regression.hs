-- This Source Code Form is subject to the terms of the Mozilla Public
-- License, v. 2.0. If a copy of the MPL was not distributed with this
-- file, You can obtain one at https://mozilla.org/MPL/2.0/.
-- Copyright (c) 2026 2youg1 and the sprawling contributors

{-# LANGUAGE GADTs #-}
{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE OverloadedStrings #-}

-- | What this adversary delivers: a Rust test.
--
-- A counterexample that stays in Haskell is knowledge this repository does not
-- have. Rendering it as a test beside the code it accuses moves the knowledge
-- into the language that ships, and leaves nothing here that could grow into a
-- second authority.
--
-- The emitted test enters by the door `channels::server` uses —
-- `assembly::RunWorker::handle` — rather than by a socket. The adversary attacks
-- the binary over the wire because that is where an agent stands; the
-- regression it hands back runs in-process because a committed test should not
-- need a port, and the policy under accusation is the same on both paths.
--
-- The output has to survive `cargo fmt --check`, so this module agrees with
-- rustfmt rather than merely producing valid Rust.
module Sprawling.Regression
  ( sequenced
  , coherent
  , render
  ) where

import Data.List (sort)
import Data.Map.Strict qualified as Map
import Data.Typeable (Typeable)
import Data.Text (Text)
import Data.Text qualified as Text
import Test.QuickCheck.StateModel

import Sprawling.Door (Scope (..), Template (..))
import Sprawling.Frame (Code (..))
import Sprawling.Model

-- | Builds a runnable trace out of a list of actions.
--
-- The polarity of each step is decided by the model rather than declared here,
-- so a hand-written trace cannot claim that something succeeds where the model
-- says it must fail.
sequenced :: [Any (Action World)] -> Actions World
sequenced = Actions . go initialState 1
  where
    go _ _ [] = []
    go world index (Some act : rest) =
      let positive = refusal world act == Nothing
          bound = mkVar index
          polar = ActionWithPolarity act (if positive then PosPolarity else NegPolarity)
          next
            | positive = nextState world act bound
            | otherwise = failureNextState world act
       in (bound := polar) : go next (index + 1) rest

-- | Whether every step of a trace is one the model admits at that point.
--
-- A hand-written trace drifts as the model learns more. This turns that drift
-- into a failing test rather than a silently skipped one.
coherent :: Actions World -> Bool
coherent (Actions steps) = go initialState steps
  where
    go _ [] = True
    go world ((bound := ActionWithPolarity act polarity) : rest) =
      admits world act polarity && go (advance world bound act polarity) rest
    admits world act PosPolarity = refusal world act == Nothing
    admits world act NegPolarity = validFailingAction world act

advance :: (Typeable a) => World -> Var a -> Action World a -> Polarity -> World
advance world bound act PosPolarity = nextState world act bound
advance world _ act NegPolarity = failureNextState world act

-- | Renders a trace as a Rust integration test.
render :: Text -> Actions World -> Text
render name actions@(Actions steps) =
  Text.unlines (preamble name actions <> walked initialState 1 steps <> closing actions)
  where
    -- The blank line separates two steps, so the last step does not get one.
    -- rustfmt deletes an empty line before a closing brace, and a renderer that
    -- emitted one would produce a file `cargo fmt --check` rejects — which is
    -- to say a deliverable nobody can commit.
    walked _ _ [] = []
    walked world index ((bound := ActionWithPolarity act polarity) : rest) =
      step world index act polarity
        <> ["" | not (null rest)]
        <> walked (advance world bound act polarity) (index + 1) rest

preamble :: Text -> Actions World -> [Text]
preamble name actions =
  [ "// This Source Code Form is subject to the terms of the Mozilla Public"
  , "// License, v. 2.0. If a copy of the MPL was not distributed with this"
  , "// file, You can obtain one at https://mozilla.org/MPL/2.0/."
  , "// Copyright (c) 2026 2youg1 and the sprawling contributors"
  , ""
  , "//! A trace the adversary found, kept here so this repository remembers it."
  , "//!"
  , "//! Written by `adversary/src/Sprawling/Regression.hs` and compared against it"
  , "//! byte for byte. Change the trace there; changing it here turns the adversary"
  , "//! red, which is exactly what should happen when the two disagree."
  , "//!"
  , "//! The adversary drives the shipped binary over the wire. This enters by the"
  , "//! same door `channels::server` does, so the trace runs without a port."
  , ""
  , "#![allow("
  , "    clippy::unwrap_used,"
  , "    clippy::expect_used,"
  , "    clippy::panic,"
  , "    clippy::indexing_slicing,"
  , "    reason = \"test code\""
  , ")]"
  , ""
  ]
    <> imports actions
    <> [ ""
       , "#[test]"
       , "fn " <> name <> "() {"
       , "    let dir = tempfile::tempdir().unwrap();"
       ]
    <> [ "    let raised = assembly::init_city(dir.path()).unwrap();" | looking actions
       ]
    <> [ "    assembly::init_city(dir.path()).unwrap();" | not (looking actions)
       ]
    <> [ ""
       , "    // The vault is the in-session one: a test that reached the platform"
       , "    // credential service would write to the machine running it."
       , "    let mut worker = assembly::RunWorker::new("
       , "        dir.path(),"
       , "        gateway::Custodian::in_memory(),"
       , "        runtime::diagnostics::Diagnostics::off(),"
       , "    )"
       , "    .unwrap();"
       , ""
       ]

-- | Exactly the imports the emitted test uses.
--
-- Not one more: the workspace refuses an unused import, so a renderer that
-- always emitted the same header would produce a file nobody could commit.
imports :: Actions World -> [Text]
imports actions =
  [taken "kernel" wanted]
    <> ["use sprawling::assembly;"]
  where
    taken from [only] = "use " <> from <> "::" <> only <> ";"
    taken from names = "use " <> from <> "::{" <> Text.intercalate ", " names <> "};"
    wanted =
      sort . concat $
        [ ["Address" | addressing actions]
        , ["AxCode" | refusing actions]
        , ["IdemKey", "RunId", "Seq"]
        ]

closing :: Actions World -> [Text]
closing actions
  | looking actions = ["}", "", standingHelper]
  | otherwise = ["}"]

-- | The helper a `Look` step calls, emitted only when one is in the trace.
--
-- It reads the Ledger rather than the worker's own fields, because what a city
-- is, is what its history says.
-- Joined rather than unlined: this is one element of the outer 'Text.unlines',
-- and a trailing newline of its own would end the file on a blank line.
standingHelper :: Text
standingHelper =
  Text.intercalate "\n"
    [ "/// The addresses this city's history says stand, in the order it wrote them."
    , "///"
    , "/// The address is read from the payload rather than from the envelope: a"
    , "/// city-level record is written against `RunId::CITY` with no address of its"
    , "/// own, so an envelope check would pass for the wrong reason."
    , "fn standing(ledger: &std::path::Path) -> Vec<String> {"
    , "    let verified = runtime::replay::verify_ledger_dir(ledger).unwrap();"
    , "    let mut addresses: Vec<String> = verified"
    , "        .raw_lines()"
    , "        .iter()"
    , "        .filter_map(|line| {"
    , "            let record = kernel::EventRecord::parse_line(line).unwrap();"
    , "            if record.kind() != kernel::EventKind::BuildingCreated {"
    , "                return None;"
    , "            }"
    , "            record"
    , "                .data()"
    , "                .as_map()"
    , "                .get(\"addr\")"
    , "                .and_then(serde_json::Value::as_str)"
    , "                .map(str::to_owned)"
    , "        })"
    , "        .collect();"
    , "    addresses.sort();"
    , "    addresses"
    , "}"
    ]

step :: World -> Int -> Action World a -> Polarity -> [Text]
step world index act = \case
  PosPolarity -> succeeded world index act
  NegPolarity -> refused world index act

succeeded :: World -> Int -> Action World a -> [Text]
succeeded world index = \case
  Look ->
    [ "    assert_eq!("
    , "        standing(&raised.ledger_dir),"
    , "        vec![" <> Text.intercalate ", " (map owned (sort (Map.keys (worldBuildings world)))) <> "]"
    , "    );"
    ]
  act ->
    ["    worker"]
      <> handed 8 index act
      <> ["        .unwrap();"]
  where
    owned addr = quoted addr <> ".to_owned()"

-- | A refusal, in two statements rather than one.
--
-- Putting the call inside `assert_eq!` would leave its shape to whatever
-- rustfmt does inside a macro; taking the error out first keeps the formatting
-- predictable and the assertion readable.
refused :: World -> Int -> Action World a -> [Text]
refused world index act =
  ["    let " <> complaint index <> " = worker"]
    <> handed 8 index act
    <> [ "        .unwrap_err();"
       , "    assert_eq!(*" <> complaint index <> ".code(), AxCode::" <> owed (refusal world act) <> ");"
       , "    // A refusal is a promise in three parts; a code with no way forward"
       , "    // keeps only one of them."
       , "    assert!(!" <> complaint index <> ".recovery().is_empty());"
       ]
  where
    owed (Just code) = variant code
    owed Nothing = "TheAdversaryOwesNoCodeHereWhichIsItsOwnBug"

-- | The Rust name of a code the door prints.
--
-- Written out rather than derived from the string, so a code this adversary has
-- never asserted on cannot be rendered into a name that does not exist.
variant :: Code -> Text
variant (Code code) = case code of
  "E_INVALID_ARGS" -> "InvalidArgs"
  "E_GATE_DENIED" -> "GateDenied"
  "E_CONFIG_INVALID" -> "ConfigInvalid"
  "E_WIRE_MISMATCH" -> "WireMismatch"
  other -> "UnknownToTheRenderer_" <> other

-- | One `handle` call, laid out the way rustfmt lays a method chain out.
handed :: Int -> Int -> Action World a -> [Text]
handed column index act =
  [margin <> ".handle(channels::Command::" <> verb act <> " {"]
    <> [margin <> "    " <> field | field <- fields index act]
    <> [margin <> "})"]
  where
    margin = Text.replicate column " "

verb :: Action World a -> Text
verb = \case
  Raise {} -> "CreateBuilding"
  Work {} -> "Dispatch"
  Stop _ -> "Halt"
  Resume _ -> "Release"
  Seize -> "Takeover"
  Look -> "Look is read from the Ledger and never handed to a worker"

fields :: Int -> Action World a -> [Text]
fields index = \case
  Raise addr template ->
    [ "addr: Address::parse(" <> quoted addr <> ").unwrap(),"
    , "template: channels::TemplateName::parse(" <> quoted (templateName template) <> ").unwrap(),"
    , idem index
    ]
  Work addr session ->
    [ "addr: Address::parse(" <> quoted addr <> ").unwrap(),"
    , "task: \"say something\".to_owned(),"
    , "goal: \"an answer\".to_owned(),"
    , "mode: channels::ModeTag::parse(\"build\").unwrap(),"
    , "budget: kernel::BudgetCap {"
    , "    usd: kernel::UsdMicros::new(1_000),"
    , "    tokens: kernel::Tokens::new(1_000),"
    , "},"
    , idem index
    , "session: Some(kernel::SessionName::parse(" <> quoted session <> ").unwrap()),"
    , "effort: None,"
    ]
  Stop scope -> ["scope: " <> scopeOf scope <> ",", idem index]
  Resume scope -> ["scope: " <> scopeOf scope <> ",", idem index]
  Seize ->
    [ "run: RunId::from_bytes([1u8; 16]),"
    , idem index
    ]
  Look -> []

-- | A distinct key per step, derived the way the product derives one.
--
-- Every state-changing command owns an `IdemKey`, so two steps sharing one
-- would make the second a replay of the first rather than a new action.
idem :: Int -> Text
idem index =
  "idem: IdemKey::derive(&RunId::CITY, Seq::FIRST, b\"step" <> Text.pack (show index) <> "\"),"

scopeOf :: Scope -> Text
scopeOf City = "channels::HaltScope::City"
scopeOf (Building addr) =
  "channels::HaltScope::Building(Address::parse(" <> quoted addr <> ").unwrap())"

templateName :: Template -> Text
templateName Minimal = "minimal"
templateName Confidential = "confidential"

complaint :: Int -> Text
complaint index = "refused" <> Text.pack (show index)

-- | Whether any step names an address, and so needs `Address`.
addressing :: Actions World -> Bool
addressing (Actions steps) = any named steps
  where
    named (_ := polar) = case polarAction polar of
      Raise {} -> True
      Work {} -> True
      Stop (Building _) -> True
      Resume (Building _) -> True
      _ -> False

-- | Whether any step is expected to be refused, and so needs `AxCode`.
refusing :: Actions World -> Bool
refusing (Actions steps) = any negative steps
  where
    negative (_ := polar) = polarity polar == NegPolarity

-- | Whether any step reads the city back, and so needs the helper.
looking :: Actions World -> Bool
looking (Actions steps) = any reading steps
  where
    reading (_ := polar) = case polarAction polar of
      Look -> True
      _ -> False

quoted :: Text -> Text
quoted text = "\"" <> Text.concatMap escaped text <> "\""
  where
    escaped '"' = "\\\""
    escaped '\\' = "\\\\"
    escaped character = Text.singleton character
