-- This Source Code Form is subject to the terms of the Mozilla Public
-- License, v. 2.0. If a copy of the MPL was not distributed with this
-- file, You can obtain one at https://mozilla.org/MPL/2.0/.
-- Copyright (c) 2026 2youg1 and the sprawling contributors

{-# LANGUAGE OverloadedStrings #-}

-- | Six groups, in the order that loses the least time when something breaks.
--
-- The renderer is checked first because it takes milliseconds and because a
-- broken deliverable makes every counterexample below it worthless. The door's
-- own contract comes next, then random traces, then the directed attack, then
-- what the history says about itself, then the disk's simplest lie.
--
-- With no binary to drive, this exits successfully and says so. A gate that
-- could not run is not a gate that failed, and treating it as one is how a
-- Haskell toolchain would end up blocking Rust contributors.
module Main (main) where

import Control.Monad.Reader (ReaderT, asks, liftIO, runReaderT)
import Data.List (isInfixOf)
import Data.Text qualified as Text
import Data.Text.IO qualified as Text
import System.Directory (doesFileExist)
import System.Environment (lookupEnv)
import System.FilePath ((</>))
import Test.QuickCheck (Property, ioProperty, withMaxSize, withNumTests)
import Test.QuickCheck.DynamicLogic (forAllDL)
import Test.QuickCheck.Monadic (PropertyM, monadic, run)
import Test.QuickCheck.StateModel (Actions, Any (..), runActions)
import Test.Tasty (TestTree, defaultMain, localOption, testGroup)
import Test.Tasty.HUnit (assertBool, assertEqual, testCase)
import Test.Tasty.QuickCheck (testProperty)
import Test.Tasty.Runners (NumThreads (..))

import Sprawling.Door (Answer (..), Door, Scope (..), Template (..))
import Sprawling.Door qualified as Door
import Sprawling.Frame (Code (..), Complaint (..))
import Sprawling.Frame qualified as Frame
import Sprawling.Ground
import Sprawling.Model
import Sprawling.Regression (coherent, render, sequenced)

main :: IO ()
main = do
  found <- Door.discover
  case found of
    Nothing ->
      putStrLn
        "skipped: no sprawling binary to drive. Run `just adversary`, or set SPRAWLING_BIN."
    Just door -> defaultMain (properties door)

-- | One group at a time, never several at once.
--
-- A served city owns a port, a directory and a history, and two groups running
-- together contend for all three. That contention does not merely slow a run
-- down: it was measured turning the open finding below green, because the
-- losing ground's readiness probe reached the winning ground's city and drove a
-- trace against somebody else's history. An adversary whose answer depends on
-- which thread won is worth less than no adversary, since it reports a defect
-- as fixed. The whole suite costs under a minute serially, which is the right
-- price for an answer that means the same thing every time.
properties :: Door -> TestTree
properties door =
  localOption (NumThreads 1)
    . testGroup
      "adversary"
    $
    [ testCase "the committed Rust test is what this adversary renders" deliverable
    , testGroup
        "the door"
        [ testCase "a credential cannot be spelled onto the wire" (sealed door)
        , testCase "the exit code says what the frames say" (exitCodeMeansWhatItSays door)
        , testCase "a verb the city cannot perform still answers" (notBuiltStillAnswers door)
        ]
    , testProperty "a city admits exactly what its rules admit" (traces door)
    , testProperty "a halted city takes no work until it is released" (halting door)
    , testProperty "history reads back as one unbroken chain" (chained door)
    , testGroup
        "the disk lies"
        [ testCase "a corrupted record is refused, not believed" (tampering door)
        , testCase "a torn tail is recovered, not refused" (torn door)
        , testCase "a record written twice is refused" (doubled door)
        ]
    , -- Two symptoms of one cause. They were an open finding until `dispatch_in`
      -- was made to judge before it writes (card V3.51); they stay together and
      -- keep their names, because what they now defend is the ordering that fix
      -- established rather than any one line of it.
      testGroup
        "a refusal costs nothing"
        [ testCase "a refused dispatch leaves nothing on disk" (nothingBehind door)
        , testCase "the city lists only what it raised" (listsOnlyRaised door)
        ]
    , -- Alone, so that a red says which defect it is. Green the day something
      -- consults the key the wire makes every command carry.
      testGroup
        "open finding: the idempotency key nothing reads"
        [testCase "a key the city has seen does not do the work again" (keyUsedTwice door)]
    ]

-- | Rule four of the SPEC, made mechanical.
--
-- The renderer is the authority for that Rust file, and the file is the proof
-- that what the renderer emits compiles and passes. Neither can move without
-- the other, and neither is a place where a rule could be restated.
deliverable :: IO ()
deliverable = do
  assertBool "the committed trace is not one the model admits" (coherent remembered)
  accepting <- lookupEnv "SPRAWLING_ACCEPT"
  if accepting == Just "1"
    then Text.writeFile path rendered
    else do
      there <- doesFileExist path
      assertBool ("there is no " <> path <> "; rerun with SPRAWLING_ACCEPT=1") there
      onDisk <- Text.readFile path
      assertEqual "the committed Rust test is not what this adversary renders" rendered onDisk
  where
    rendered = render "a_halted_city_names_the_halt_and_not_the_configuration" remembered
    path = ".." </> "crates" </> "sprawling" </> "tests" </> "from_adversary.rs"

-- | The trace this adversary keeps, and why it is this one.
--
-- With nothing attached, a dispatch stops at configuration and the caller is
-- told to go and attach a provider. Halt the city and the same dispatch must
-- stop one door earlier, at the gate, and say so — because the recovery line
-- is the third part of what a refusal promises, and "attach a provider" is
-- advice that does not lift a halt.
--
-- The last step is the one that would have gone wrong had those two guards
-- been written in the other order, and neither refusal is a value anybody
-- computed: both are promises the door makes to whoever is driving it.
remembered :: Actions World
remembered =
  sequenced
    [ Some (Raise "acme" Minimal)
    , Some (Work "acme" "one")
    , Some (Stop City)
    , Some (Work "acme" "two")
    ]

-- | `Sealed<T>` has no `Serialize`, and the wire's secret carrier is
-- uninhabited — so the frame is refused before a socket is even opened.
--
-- Asserted from outside because that is where it matters: the compile-time
-- half of this property is already proved by a trybuild case inside the
-- repository, and what nobody there can check is what an agent sees when it
-- tries anyway.
sealed :: Door -> IO ()
sealed door = withGround door $ \ground -> do
  answered <-
    Door.askRaw
      door
      (portOf ground)
      "{\"command\":{\"put_secret\":{\"realm\":\"r\",\"name\":\"n\",\"value\":\"leak\"}}}"
  case answered of
    Denied complaint -> do
      assertEqual
        "a credential frame was refused for the wrong reason"
        (Code "E_WIRE_MISMATCH")
        (complaintCode complaint)
      assertBool
        "the refusal repeated the value it was refusing to carry"
        (not ("leak" `isInfixOf` Text.unpack (complaintSubject complaint)))
    other -> assertBool ("a credential reached the city: " <> show other) False

-- | What the door's own documentation promises: exit 1 when the city refused.
--
-- The two halves have to agree, because an agent driving this branches on the
-- exit code and never reads the frames. adversary-SPEC section 4 records the
-- measurement that made this worth asserting: the code means "no refusal
-- arrived inside the quiet window", which is not the same statement.
exitCodeMeansWhatItSays :: Door -> IO ()
exitCodeMeansWhatItSays door = withGround door $ \ground -> do
  let key = case Door.idemKeys of
        (first : _) -> first
        [] -> error "idemKeys is infinite"
  refusedOne <- Door.ask door (portOf ground) (Door.CreateBuilding ".sprawling" Minimal key)
  case refusedOne of
    Denied _ -> pure ()
    other -> assertBool ("the reserved subtree was not defended: " <> show other) False
  accepted <- Door.ask door (portOf ground) Door.CityView
  case accepted of
    Accepted _ -> pure ()
    other -> assertBool ("a plain query did not answer: " <> show other) False

-- | Five verbs the wire spells and this city does not perform.
--
-- A refusal that names the verb and offers a way forward is the promise; going
-- silent, or answering with a code that means something else, is not. This is
-- the property that has to hold *before* anybody wires a control to them.
notBuiltStillAnswers :: Door -> IO ()
notBuiltStillAnswers door = withGround door $ \ground -> do
  let key = case drop 1 Door.idemKeys of
        (second : _) -> second
        [] -> error "idemKeys is infinite"
  answered <-
    Door.ask door (portOf ground) (Door.Takeover "00000000-0000-7000-8000-000000000000" key)
  case answered of
    Denied complaint -> do
      assertEqual
        "a verb that is not built answered with the wrong code"
        (Code "E_WIRE_MISMATCH")
        (complaintCode complaint)
      assertBool
        "a verb that is not built refused without a way forward"
        (not (Text.null (complaintRecovery complaint)))
    other -> assertBool ("a verb that is not built said: " <> show other) False

-- | Any trace at all, against a city raised for it and thrown away after.
--
-- Sample counts and trace lengths are both deliberately small. Each sample
-- raises a city, serves it, and spends one process per action, so the honest
-- unit of cost here is seconds per sample rather than samples per second; the
-- shrinking is what finds the defect, and it runs on whichever sample failed
-- first.
--
-- The length bound is the load-bearing one. An action costs 0.3 s measured, and
-- QuickCheck's default schedule would have grown these traces past seventy
-- actions, which buys twenty-two seconds of the same three verbs repeated.
-- What finds a defect here is which verbs meet, not how many times.
traces :: Door -> Actions World -> Property
traces door actions =
  withMaxSize 12 . withNumTests 4 . driving door $ do
    _ <- runActions actions
    pure True

-- | Any prefix, one halt, any suffix, and work that must still be refused.
halting :: Door -> Property
halting door = withNumTests 3 (forAllDL haltIsHonoured (traces door))

-- | After any trace, the history the city wrote is one unbroken chain.
--
-- Two statements in one, and neither of them recomputes anything: the door's
-- own offline verifier is asked whether the chain holds, and the pushed
-- records are checked to be dense and strictly increasing. A city that forked
-- its own history, or skipped a number, would have a history nobody can replay
-- — and replay is what `sprawling resume` is built on.
chained :: Door -> Actions World -> Property
chained door actions =
  withMaxSize 12 . withNumTests 3 . driving door $ do
    _ <- runActions actions
    verified <- run $ do
      ground <- asks kitGround
      liftIO (Door.verify door (ledgerOf ground))
    pure (either (const False) (const True) verified)

-- | What a refusal costs on disk, which should be nothing.
--
-- ARCHITECTURE section 5 makes the ordering load-bearing — every effect becomes
-- an event first — and `dispatch_in` opens by saying so in as many words: a
-- halted city that laid a job file down would leave a task in a room no run
-- ever opened. The halt is checked before the write and honours that. The model
-- tag is resolved after it, and does not.
--
-- So this dispatches to an address nobody raised. The city refuses. What it
-- leaves behind is a building directory, a room inside it and a `JOB.md`, none
-- of which any record in the one history mentions — files a person can see that
-- the city cannot account for.
nothingBehind :: Door -> IO ()
nothingBehind door = withGround door $ \ground -> do
  let key = case drop 3 Door.idemKeys of
        (fourth : _) -> fourth
        [] -> error "idemKeys is infinite"
  before <- tree ground
  answered <- Door.ask door (portOf ground) (Door.Dispatch "acme" "one" key)
  case answered of
    Accepted _ ->
      assertBool "a dispatch with no model attached was accepted" False
    Quiet -> assertBool "a dispatch with no model attached said nothing" False
    Denied _ -> do
      after <- tree ground
      assertEqual
        "a refused dispatch wrote into the city"
        before
        after

-- | The same defect seen from the other side: through a query.
--
-- `city_view` answers by scanning directories, so the room a refused dispatch
-- left behind is reported as a building. Nothing raised it, no
-- `building_created` record mentions it, and it has no rules of its own — but
-- a person reading the city sees it standing there.
listsOnlyRaised :: Door -> IO ()
listsOnlyRaised door = withGround door $ \ground -> do
  let key = case drop 4 Door.idemKeys of
        (fifth : _) -> fifth
        [] -> error "idemKeys is infinite"
  _ <- Door.ask door (portOf ground) (Door.Dispatch "gamma" "one" key)
  answered <- Door.ask door (portOf ground) Door.CityView
  case answered of
    Accepted frames -> case [body | Frame.Answered "city" body <- frames] of
      (body : _) ->
        assertEqual
          "the city listed a building nobody raised"
          (Just [])
          (Frame.cityBuildings body)
      [] -> assertBool "the city did not answer with a city" False
    other -> assertBool ("the city could not be read: " <> show other) False

-- | A tail torn off mid-record, which the product recovers from on purpose.
--
-- The one hostile action here whose answer is "this is fine". `memory::jsonl`
-- treats an unfinished last line as a write that did not land, so the history
-- reads back one record shorter and still verifies. Asserting the relation
-- rather than a length: what is owed is that the chain still holds and that it
-- did not somehow grow.
torn :: Door -> IO ()
torn door = withGround door $ \ground -> do
  let key = case drop 5 Door.idemKeys of
        (sixth : _) -> sixth
        [] -> error "idemKeys is infinite"
  _ <- Door.ask door (portOf ground) (Door.CreateBuilding "acme" Minimal key)
  whole <- Door.verify door (ledgerOf ground)
  tear ground
  recovered <- Door.verify door (ledgerOf ground)
  case (whole, recovered) of
    (Left refusedWhy, _) ->
      assertBool ("a clean history did not verify: " <> show refusedWhy) False
    (Right _, Left refusedWhy) ->
      assertBool
        ("a torn tail was refused rather than recovered: " <> show refusedWhy)
        False
    (Right before, Right after) ->
      assertBool
        ("a torn history verified further than the whole one: " <> show after <> " past " <> show before)
        (after <= before)

-- | One record written a second time, which the reader must not accept.
--
-- A repeated line carries a sequence number and a previous-hash that already
-- belong to the line above it, so a history that verified would be one two
-- different accounts of the past could explain — and `sprawling resume` builds
-- what it does on there being one.
doubled :: Door -> IO ()
doubled door = withGround door $ \ground -> do
  let key = case drop 6 Door.idemKeys of
        (seventh : _) -> seventh
        [] -> error "idemKeys is infinite"
  _ <- Door.ask door (portOf ground) (Door.CreateBuilding "acme" Minimal key)
  duplicate ground
  after <- Door.verify door (ledgerOf ground)
  case after of
    Left _ -> pure ()
    Right verified ->
      assertBool
        ("a history with a record written twice verified as far as " <> show verified)
        False

-- | The same key on the same command twice, which must happen once.
--
-- Every state-changing command on this wire carries an `IdemKey`, and a key
-- exists to make a retry harmless: a client that sent a command, lost its
-- connection and sent it again must not have done it twice.
--
-- Observed through the door and nowhere else. The history's own verifier is
-- asked how far the chain runs before and after the second send, so what is
-- asserted is that the two readings agree — no sequence number is predicted and
-- no rule is recomputed here.
--
-- `kernel::gate::dedup` is the door that would enforce this, and outside its own
-- tests nothing calls it.
keyUsedTwice :: Door -> IO ()
keyUsedTwice door = withGround door $ \ground -> do
  let key = case drop 7 Door.idemKeys of
        (eighth : _) -> eighth
        [] -> error "idemKeys is infinite"
  _ <- Door.ask door (portOf ground) (Door.Halt City key)
  once <- Door.verify door (ledgerOf ground)
  _ <- Door.ask door (portOf ground) (Door.Halt City key)
  twice <- Door.verify door (ledgerOf ground)
  case (once, twice) of
    (Right before, Right after) ->
      assertEqual
        "a command carrying a key the city had already seen was performed a second time"
        before
        after
    _ ->
      assertBool
        ("a history this test wrote did not verify: " <> show once <> " then " <> show twice)
        False

-- | The simplest lie a disk can tell: one changed byte.
--
-- What the reader must never do is believe it. Nothing in the command asks for
-- a check, which is the point — verification is not an option a caller can
-- forget.
tampering :: Door -> IO ()
tampering door = withGround door $ \ground -> do
  let key = case drop 2 Door.idemKeys of
        (third : _) -> third
        [] -> error "idemKeys is infinite"
  _ <- Door.ask door (portOf ground) (Door.CreateBuilding "acme" Minimal key)
  before <- Door.verify door (ledgerOf ground)
  case before of
    Left refusedWhy -> assertBool ("a clean history did not verify: " <> show refusedWhy) False
    Right _ -> do
      corrupt ground
      after <- Door.verify door (ledgerOf ground)
      case after of
        Left complaint ->
          assertBool
            ("a corrupted history was refused, but not as corruption: " <> show complaint)
            ("E_CAS_CORRUPT" `isInfixOf` Text.unpack complaint)
        Right lines_ ->
          assertBool
            ("a corrupted history verified as " <> show lines_ <> " good line(s)")
            False

-- | Runs a trace against the real program in a city that is thrown away.
driving :: Door -> PropertyM (ReaderT Kit IO) Bool -> Property
driving door act =
  monadic (\attempt -> ioProperty (withGround door (runReaderT attempt . Kit door))) act
