-- This Source Code Form is subject to the terms of the Mozilla Public
-- License, v. 2.0. If a copy of the MPL was not distributed with this
-- file, You can obtain one at https://mozilla.org/MPL/2.0/.
-- Copyright (c) 2026 2youg1 and the sprawling contributors

{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE GADTs #-}
{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE TypeFamilies #-}

-- | What has to hold between what a city was told and what it will admit.
--
-- The model remembers only what a person would remember: which buildings
-- stand, what is halted, and whether any provider has ever been attached. It
-- never predicts a sequence number, a chain hash, a timestamp or an
-- `IdemKey` — those are rules, they already have an authority in Rust, and
-- restating one here is how a second authority begins.
--
-- What it does assert is which failure a caller sees. A stable code is part of
-- the door's contract rather than a derived value, so pinning it pins the
-- promise the product makes to the agent on the other side.
module Sprawling.Model
  ( World (..)
  , Action (..)
  , Kit (..)
  , Attempt
  , refusal
  , standing
  , haltIsHonoured
  , reserved
  ) where

import Control.Monad (when)
import Control.Monad.Reader (ReaderT, asks, liftIO)
import Data.List (sort)
import Data.Map.Strict (Map)
import Data.Map.Strict qualified as Map
import Data.Set (Set)
import Data.Set qualified as Set
import Data.Text (Text)
import Data.Text qualified as Text
import Test.QuickCheck qualified as QC
import Test.QuickCheck.DynamicLogic (DL, DynLogicModel, action, anyActions_, failingAction, getModelStateDL)
import Test.QuickCheck.StateModel

import Sprawling.Frame (Code (..), Complaint (..), Frame)
import Sprawling.Frame qualified as Frame
import Sprawling.Door (Answer (..), Door, IdemKey, Scope (..), Template (..))
import Sprawling.Door qualified as Door
import Sprawling.Ground

-- | Everything a person could know after a trace.
--
-- `worldMinted` is a counter, not a prediction: it exists so that each action
-- carries a key nothing else carried, which is what keeps the product's
-- deduplication from turning a second distinct action into a replay of the
-- first.
data World = World
  { worldBuildings :: Map Text Template
  , worldHalted :: Set Scope
  , worldAttached :: Bool
  , worldMinted :: Int
  }
  deriving stock (Show)

instance HasVariables World where
  getAllVariables _ = mempty

instance HasVariables (Action World a) where
  getAllVariables _ = mempty

instance StateModel World where
  data Action World a where
    -- | Lay out a building at an address.
    Raise :: Text -> Template -> Action World ()
    -- | Ask for work in one, under a session name.
    Work :: Text -> Text -> Action World ()
    -- | Stop admitting work in a scope.
    Stop :: Scope -> Action World ()
    -- | Admit it again.
    Resume :: Scope -> Action World ()
    -- | Take the wheel from a run — a verb the wire spells and the city
    -- does not perform.
    Seize :: Action World ()
    -- | Read the city back.
    Look :: Action World [Text]

  initialState =
    World
      { worldBuildings = Map.empty
      , worldHalted = Set.empty
      , worldAttached = False
      , worldMinted = 0
      }

  -- `Look` is reachable in a hand-written trace and absent from this
  -- generator, which is not an oversight. A refused dispatch leaves a
  -- directory behind (adversary-SPEC section 4), and `city_view` scans
  -- directories, so a random trace containing one would fail every later
  -- `Look` for a cause that has nothing to do with the step it landed on.
  -- Generating it would therefore report one defect as many, and hide the
  -- next one behind it. The claim itself is asserted once, by name, in the
  -- test suite.
  arbitraryAction _ world =
    QC.frequency
      [ (4, Some <$> (Raise <$> anAddress <*> pure Minimal))
      , (5, Some <$> (Work <$> anAddress <*> aSession))
      , (3, Some <$> (Stop <$> aScope))
      , (2, Some <$> (Resume <$> aScope))
      , (1, pure (Some Seize))
      ]
    where
      -- The reserved subtree is in the generator on purpose: an address a
      -- write domain may never reach is exactly the address an adversary
      -- would try, and leaving it out would make the property vacuous.
      anAddress = QC.elements (cast <> [".sprawling", ".sprawling/books"])
      aSession = QC.elements ["one", "two"]
      aScope =
        QC.frequency
          [ (2, pure City)
          , (1, Building <$> QC.elements (Map.keys (worldBuildings world) <> ["acme"]))
          ]

  -- A precondition that always held would make every action a positive one, so
  -- `failingAction` could never be scheduled and the directed attack below
  -- would report "failed precondition" instead of running. The two halves have
  -- to partition: an action is positive exactly when no refusal is owed.
  precondition world act = refusal world act == Nothing

  -- An action the model expects to be refused is worth running, because the
  -- refusal is the promise. Running it as a negative action is how the door's
  -- error codes get tested at all.
  validFailingAction world act = refusal world act /= Nothing

  nextState world act _ = case act of
    Raise addr template
      | refusal world act == Nothing ->
          minted world {worldBuildings = Map.insert addr template (worldBuildings world)}
      | otherwise -> minted world
    Work _ _ -> minted world
    Stop scope -> minted world {worldHalted = Set.insert scope (worldHalted world)}
    Resume scope -> minted world {worldHalted = Set.delete scope (worldHalted world)}
    Seize -> minted world
    Look -> world
    where
      minted next = next {worldMinted = worldMinted world + 1}

deriving stock instance Show (Action World a)

deriving stock instance Eq (Action World a)

instance DynLogicModel World

-- | Whether an address names something inside a city's own reserved subtree.
--
-- `kernel::address` answers this with one predicate over the segments rather
-- than with a list of protected names, and this mirrors the shape of that
-- rule without recomputing any of its parsing.
reserved :: Text -> Bool
reserved addr = ".sprawling" `elem` Text.splitOn "/" addr

-- | Whether work is admitted at an address right now.
standing :: World -> Text -> Bool
standing world addr =
  not (Set.member City (worldHalted world))
    && not (Set.member (Building addr) (worldHalted world))

-- | The refusal the model says a caller must see, if any.
--
-- The order of the guards is the order the program checks in, and getting it
-- wrong would not weaken the property — it would make the adversary demand a
-- different failure than the one the caller is entitled to. Two orderings are
-- load-bearing and are the reason this function is written as a chain rather
-- than as a set of independent tests:
--
--   * A halted city answers "halted", not "no model is chosen". Swapping them
--     would send a person to attach a provider when what stopped their work
--     was a halt they can lift — and the recovery line is the third part of
--     what `AxError` promises.
--   * A reserved address is refused for being reserved before it is refused
--     for being occupied, because nothing may occupy it in the first place.
--   * For work, the halt outranks the address as well. This one is written
--     from measurement rather than from taste: the first draft assumed a
--     malformed address is judged first, and a halted city asked for work at
--     `.sprawling/books` answers `E_GATE_DENIED`. The product is consistent
--     about it — the same address under no halt answers `E_INVALID_ARGS`, and
--     `create_building` at a reserved address answers `E_INVALID_ARGS` even
--     while the city is halted, because laying out a building is not work and
--     the halt does not cover it.
refusal :: World -> Action World a -> Maybe Code
refusal world = \case
  Raise addr _
    | reserved addr -> Just invalid
    | Map.member addr (worldBuildings world) -> Just invalid
    | otherwise -> Nothing
  Work addr _
    -- The halt is the outermost gate on work, ahead of the address itself.
    | not (standing world addr) -> Just (Code "E_GATE_DENIED")
    | reserved addr -> Just invalid
    -- Nothing has been attached, so no tag names a model, and every dispatch
    -- stops at configuration. adversary-SPEC section 3 records that no provider
    -- is ever attached.
    --
    -- Whether the building exists is **not** asked here, and that is a measured
    -- fact about the product rather than a simplification: `dispatch_in` checks
    -- the halt, writes the brief, and only then resolves the model tag, so an
    -- address nobody raised is refused for having no model. What that costs is
    -- asserted by `aRefusedDispatchLeavesNothingBehind` rather than smuggled
    -- into this code, because the two are different claims: this one says which
    -- refusal a caller gets, that one says what the disk looks like afterwards.
    | not (worldAttached world) -> Just (Code "E_CONFIG_INVALID")
    | otherwise -> Nothing
  Stop _ -> Nothing
  Resume _ -> Nothing
  -- Not built, and answered one verb at a time rather than by a catch-all, so
  -- the promise is that this verb refuses with a code and a way forward.
  Seize -> Just (Code "E_WIRE_MISMATCH")
  Look -> Nothing
  where
    invalid = Code "E_INVALID_ARGS"

-- | What it takes to run a trace: a built binary and a city to run it against.
data Kit = Kit
  { kitDoor :: Door
  , kitGround :: Ground
  }

type Attempt = ReaderT Kit IO

instance RunModel World Attempt where
  type Error World Attempt = Complaint

  perform world act _ = case act of
    Raise addr template -> spoken (Door.CreateBuilding addr template) (const (Just ()))
    Work addr session -> spoken (Door.Dispatch addr session) (const (Just ()))
    Stop scope -> spoken (Door.Halt scope) (const (Just ()))
    Resume scope -> spoken (Door.Release scope) (const (Just ()))
    Seize -> spoken (Door.Takeover "00000000-0000-7000-8000-000000000000") (const (Just ()))
    Look -> asked Door.CityView listed
    where
      key :: IdemKey
      key = case drop (worldMinted world) Door.idemKeys of
        (fresh : _) -> fresh
        -- `idemKeys` is infinite, so this is unreachable; it is written as a
        -- value rather than as an error because an adversary that can crash on
        -- its own bookkeeping reports its own bugs as the product's.
        [] -> Door.IdemKey "idem1-00000000000000000000000000000000"

      -- Both signatures are load-bearing: `GADTs` brings `MonoLocalBinds` with
      -- it, and without one each helper would be pinned to whichever result
      -- type it happened to be used at first.
      spoken ::
        (IdemKey -> Door.Verb) ->
        ([Frame] -> Maybe b) ->
        Attempt (Either Complaint b)
      spoken verb project = asked (verb key) project

      listed :: [Frame] -> Maybe [Text]
      listed frames = case [body | Frame.Answered "city" body <- frames] of
        (body : _) -> Frame.cityBuildings body
        [] -> Nothing

      asked :: Door.Verb -> ([Frame] -> Maybe b) -> Attempt (Either Complaint b)
      asked verb project = do
        door <- asks kitDoor
        ground <- asks kitGround
        answered <- liftIO (Door.ask door (portOf ground) verb)
        case answered of
          Denied complaint -> pure (Left complaint)
          -- Silence is not acceptance. The door waits far longer than the
          -- product's longest synchronous path, so a city that said nothing
          -- has either changed shape or stopped answering, and either is a
          -- finding rather than a step to carry on from.
          Quiet ->
            liftIO . fail $
              "the city said nothing at all to " <> show act <> "; see adversary-SPEC section 4"
          Accepted frames -> case project frames of
            Just value -> pure (Right value)
            Nothing ->
              liftIO . fail $
                "the city answered a question other than the one asked: " <> show frames

  postcondition (before, _) act _ result = case act of
    Look -> do
      let standingNow = sort (Map.keys (worldBuildings before))
      counterexamplePost ("saw " <> show result <> " where " <> show standingNow <> " stands")
      pure (result == standingNow)
    _ -> pure True

  postconditionOnFailure (before, _) act _ = \case
    Right _ -> do
      counterexamplePost ("this was accepted, and " <> show (refusal before act) <> " was owed")
      pure False
    Left complaint -> do
      counterexamplePost
        ( "refused with "
            <> show (complaintCode complaint)
            <> " where "
            <> show (refusal before act)
            <> " was owed"
        )
      -- Two assertions, because a refusal is a promise in three parts and a
      -- code with no way forward keeps only one of them.
      pure
        ( Just (complaintCode complaint) == refusal before act
            && not (Text.null (complaintRecovery complaint))
        )

-- | Any prefix, one halt, any suffix, and work that must still be refused.
--
-- Uniform random traces reach this state rarely and by accident. Naming the
-- attack and quantifying over what surrounds it is the difference between a
-- fuzzer and an adversary, and it is the reason this adversary is written in a
-- language with dynamic logic in it.
--
-- The suffix may contain a `Resume`, so what is halted is read back from the
-- model before the closing step is demanded: asserting a refusal the product
-- does not owe would be this adversary inventing a rule.
haltIsHonoured :: DL World ()
haltIsHonoured = do
  _ <- action (Raise "acme" Minimal)
  anyActions_
  _ <- action (Stop City)
  anyActions_
  world <- getModelStateDL
  when (stopped world) (failingAction (Work "acme" "one"))
  where
    stopped world = Map.member "acme" (worldBuildings world) && not (standing world "acme")
