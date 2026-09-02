-- This Source Code Form is subject to the terms of the Mozilla Public
-- License, v. 2.0. If a copy of the MPL was not distributed with this
-- file, You can obtain one at https://mozilla.org/MPL/2.0/.
-- Copyright (c) 2026 2youg1 and the sprawling contributors

{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The only place that knows a binary exists.
--
-- Everything this adversary learns, it learns by running the program an agent
-- runs, with the arguments an agent types, and reading the two streams an
-- agent reads. There is no linking, no FFI, and no shared type: what cannot be
-- reached through this module cannot be tested here, which is the point.
--
-- ARCHITECTURE section 8 says the wire is the whole API and that a second
-- client writes against it. `sprawling call` is that second client; this module
-- is a third one, written outside the repository to attack rather than to use.
module Sprawling.Door
  ( Door
  , Port (..)
  , Verb (..)
  , Template (..)
  , Scope (..)
  , Answer (..)
  , IdemKey (..)
  , discover
  , raise
  , serve
  , ask
  , askRaw
  , verify
  , idemKeys
  , quietMillis
  ) where

import Control.Concurrent (forkIO)
import Control.Exception (IOException, evaluate, try)
import Data.Aeson (Value, object, (.=))
import Data.Aeson qualified as Aeson
import Data.ByteString qualified as Strict
import Data.ByteString.Char8 qualified as StrictChar8
import Data.ByteString.Lazy qualified as Lazy
import Data.ByteString.Lazy.Char8 qualified as Char8
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.Text (Text)
import Data.Text qualified as Text
import Data.Word (Word64)
import Numeric (showHex)
import System.Directory (doesFileExist)
import System.Environment (lookupEnv)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.IO (Handle, hSetBinaryMode)
import System.Process
  ( CreateProcess (..)
  , ProcessHandle
  , StdStream (..)
  , createProcess
  , proc
  , waitForProcess
  , withCreateProcess
  )

import Sprawling.Frame (Code (..), Complaint (..), Frame (..), decodeFrame, refusalOf)

-- | A binary that has already been built.
newtype Door = Door FilePath
  deriving stock (Eq, Show)

-- | Where a city is listening.
newtype Port = Port Int
  deriving stock (Eq, Ord, Show)

-- | The deduplication key every state-changing command owns.
--
-- Minted here rather than in the model, and never predicted: the shape is the
-- door's (`idem1-` plus 32 lowercase hex digits) and what this adversary asserts
-- about it is that distinct actions carry distinct keys, not what any
-- particular key is.
newtype IdemKey = IdemKey Text
  deriving stock (Eq, Ord, Show)

-- | An endless supply of distinct keys, from a seed.
--
-- Deterministic so that a counterexample replays: the same trace mints the
-- same keys in the same order.
idemKeys :: [IdemKey]
idemKeys = [IdemKey (Text.pack ("idem1-" <> pad (showHex n ""))) | n <- [1 :: Word64 ..]]
  where
    pad digits = replicate (32 - length digits) '0' <> digits

-- | Which building template a new building is laid out from.
--
-- Two values because the product has two. `Confidential` is in the type and
-- not yet in the model's generator: a building that may not construct outward
-- tools is a second world, and modelling it without driving it would be a
-- claim this adversary has not tested.
data Template = Minimal | Confidential
  deriving stock (Eq, Ord, Show)

-- | What a halt or a release applies to.
data Scope = City | Building Text
  deriving stock (Eq, Ord, Show)

-- | One thing to ask the city to do.
data Verb
  = CityView
  | EndpointView
  | CreateBuilding Text Template IdemKey
  | Dispatch Text Text IdemKey
  | Halt Scope IdemKey
  | Release Scope IdemKey
  | Takeover Text IdemKey
  | -- | A frame this adversary writes by hand, for the cases where the point is
    -- that the door refuses to encode it at all.
    Verbatim Text
  deriving stock (Eq, Show)

-- | What the door said.
--
-- 'Quiet' is a third outcome and deliberately not a synonym for acceptance.
-- The door prints what arrives before the city has been silent for a window,
-- so a command still being served looks exactly like a command that produced
-- nothing. Collapsing the two is how a refused action gets read as a
-- successful one; adversary-SPEC section 4 records the measurement that found
-- this.
data Answer
  = Accepted [Frame]
  | Denied Complaint
  | Quiet
  deriving stock (Eq, Show)

-- | How long the door waits for the city to go silent.
--
-- The client returns only after this much silence, so the window is paid in
-- full by every single action — the first draft set it above the product's
-- longest synchronous path (`PROBE_TIMEOUT_MS`, 15 s) and thereby charged
-- thirty seconds for a query that answers in one millisecond.
--
-- What makes a short window honest is that no verb this adversary drives touches
-- a slow path: the model never attaches an endpoint, and every command it
-- sends is answered off local disk over loopback. 'Quiet' therefore means
-- "this city did not answer promptly", and the slow path's own hazard is
-- asserted by 'Sprawling.Model' having no action that walks it rather than by
-- waiting for one.
--
-- Measured rather than guessed: every command the model sends answered inside
-- one millisecond on the machine this was written on, so 250 ms is three
-- orders of margin and still two hundredths of the first draft's cost.
quietMillis :: Int
quietMillis = 250

-- | Finds the binary, preferring what the caller was told to use.
--
-- The justfile builds it and passes the path in @SPRAWLING_BIN@, so there is
-- one authority for where it is; the fallbacks exist only so that a person
-- poking at @cabal repl@ is not stopped by an environment variable.
--
-- Being told a path and not finding one there is a failure, not a skip. Nothing
-- to drive is a fact about a machine, and skipping is the right answer to it;
-- but a caller that named a binary has already built it, so an empty answer
-- there means the name was mangled on the way in — and reporting that as a
-- green run would report a suite that tested nothing as a suite that passed.
discover :: IO (Maybe Door)
discover =
  lookupEnv "SPRAWLING_BIN" >>= \case
    Just path ->
      pick [path] >>= \case
        Just door -> pure (Just door)
        Nothing ->
          fail
            ( "SPRAWLING_BIN names no file: "
                <> path
                <> "\n  On Windows this is usually a POSIX path a shell expanded; "
                <> "pass one this program can open."
            )
    Nothing -> pick (concatMap flavours ["debug", "release"])
  where
    flavours profile =
      [ ".." </> "target" </> profile </> "sprawling"
      , ".." </> "target" </> profile </> "sprawling.exe"
      ]
    pick [] = pure Nothing
    pick (candidate : rest) = do
      there <- doesFileExist candidate
      if there then pure (Just (Door candidate)) else pick rest

-- | Raises a city in a directory that does not have one.
raise :: Door -> FilePath -> IO ()
raise (Door binary) city = do
  (status, _, err) <- capture binary ["init", city]
  case status of
    ExitSuccess -> pure ()
    ExitFailure code ->
      fail ("a city could not be raised (exit " <> show code <> "): " <> Char8.unpack err)

-- | Starts serving a city, and hands back the process still running.
--
-- The caller owns its death. `--no-console` matters: without it the process
-- reads stdin, and a test harness has no keyboard to give it.
serve :: Door -> FilePath -> Port -> IO (ProcessHandle, IO String)
serve (Door binary) city (Port port) = do
  let at = "127.0.0.1:" <> show port
      spawned =
        (proc binary ["serve", city, at, "--no-console"])
          { std_in = NoStream
          , std_out = CreatePipe
          , std_err = CreatePipe
          }
  (_, out, err, handle) <- createProcess spawned
  case (out, err) of
    (Just outHandle, Just errHandle) -> do
      hSetBinaryMode outHandle True
      hSetBinaryMode errHandle True
      -- Both streams are emptied for as long as the city lives. A served city
      -- narrates on both of them, and an unread pipe stops being a pipe once
      -- the operating system's buffer fills: the city blocks inside `write`
      -- and answers nothing further. That failure arrives as a city which
      -- served for a while and then went silent, which reads exactly like a
      -- product defect and is not one.
      _ <- drained outHandle
      said <- drained errHandle
      pure (handle, said)
    _ -> fail "the city was started without the pipes it was asked for"

-- | Empties a stream in a thread of its own, keeping the tail readable.
--
-- The tail rather than the whole: diagnostics are wanted only to explain a
-- failure, and a city that ran for a thousand actions would otherwise be held
-- in memory in full. Reading is strict and chunked rather than through
-- `hGetContents`, whose laziness is what made the first draft never read
-- anything at all.
drained :: Handle -> IO (IO String)
drained stream = do
  kept <- newIORef Strict.empty
  _ <- forkIO (siphon kept)
  pure (StrictChar8.unpack <$> readIORef kept)
  where
    siphon kept = do
      arrived <- try (Strict.hGetSome stream 4096)
      case arrived of
        -- The handle closes when the city is killed, and a reader that
        -- reported that as an error would turn every teardown into a failure.
        Left (_ :: IOException) -> pure ()
        Right chunk
          | Strict.null chunk -> pure ()
          | otherwise -> do
              modifyIORef' kept (lastOf . (<> chunk))
              siphon kept
    lastOf bytes = Strict.drop (Strict.length bytes - 8000) bytes

-- | Asks one question of one city, and reads the answer.
--
-- A refusal is an answer. A line that will not parse is not: it means the wire
-- has changed shape, so this throws rather than reporting a green test against
-- a program it can no longer read.
ask :: Door -> Port -> Verb -> IO Answer
ask door port verb = askRaw door port (frameOf verb)

-- | The same, for a frame written out by hand.
askRaw :: Door -> Port -> Text -> IO Answer
askRaw (Door binary) (Port port) frame = do
  (status, out, err) <-
    capture
      binary
      [ "call"
      , Text.unpack frame
      , "--at"
      , "127.0.0.1:" <> show port
      , "--quiet-ms"
      , show quietMillis
      ]
  case status of
    -- Usage errors are this adversary's own mistake, never a fact about the city.
    ExitFailure 2 -> fail ("the door refused the invocation: " <> Char8.unpack err)
    _ -> do
      frames <- traverse readOne (Char8.lines out)
      pure (interpret (localRefusal err) frames)
  where
    readOne line = case decodeFrame line of
      Right parsed -> pure parsed
      Left reason ->
        fail $
          "the door answered in a shape this adversary cannot read: "
            <> reason
            <> "\n  asked: "
            <> Text.unpack frame
            <> "\n  said:  "
            <> Char8.unpack (Lazy.take 400 line)

-- | Sorts what came back into the three things it can mean.
--
-- A welcome on its own is silence: the handshake proves the city is there and
-- says nothing about the command that followed it.
--
-- There are two refusal channels and they mean different things. A `refusal`
-- frame on stdout is the city's judgement. A plain-text `AxError` on stderr is
-- the client's own: the frame was rejected before a socket was opened, which is
-- how `PutSecret` is refused — its wire carrier is uninhabited, so the value
-- cannot be decoded at all. Both are refusals to the caller, and collapsing
-- them would lose the fact that one never reached the city.
interpret :: Maybe Complaint -> [Frame] -> Answer
interpret local frames = case refusalOf frames of
  Just complaint -> Denied complaint
  Nothing -> case local of
    Just complaint -> Denied complaint
    Nothing -> case [frame | frame <- frames, notWelcome frame] of
      [] -> Quiet
      answered -> Accepted answered
  where
    notWelcome = \case
      Welcomed _ -> False
      _ -> True

-- | The refusal the client made on its own behalf, printed as text.
--
-- Two lines, shaped @E_CODE: cannot ACTION on SUBJECT@ and @recovery: ...@.
-- Read positionally rather than by a regular expression, because what is
-- wanted is the code and the fact that a way forward was offered, and inventing
-- a parser for the sentence would make this adversary depend on its wording.
localRefusal :: Lazy.ByteString -> Maybe Complaint
localRefusal said = case Char8.lines said of
  (headline : rest)
    | (code, remainder) <- Char8.break (== ':') headline
    , Char8.isPrefixOf "E_" code
    , not (Lazy.null remainder) ->
        Just
          Complaint
            { complaintCode = Code (Text.strip (decode code))
            , complaintAction = Text.strip (decode (Char8.drop 1 remainder))
            , complaintSubject = ""
            , complaintRecovery = recoveryIn rest
            , complaintRetriable = False
            }
  _ -> Nothing
  where
    decode = Text.pack . Char8.unpack
    recoveryIn lines_ =
      case [Char8.drop 9 line | line <- lines_, Char8.isPrefixOf "recovery:" line] of
        (line : _) -> Text.strip (decode line)
        [] -> ""

-- | Verifies a chain offline, the way `just replay` does.
--
-- Left is the refusal's first line; Right is how many lines verified. Strictly
-- read-only, which is what makes it safe to point at a ledger a city is still
-- writing.
verify :: Door -> FilePath -> IO (Either Text Word64)
verify (Door binary) ledger = do
  (status, out, err) <- capture binary ["replay", ledger]
  pure $ case status of
    ExitSuccess -> Right (tailSeq (Char8.unpack out))
    ExitFailure _ -> Left (Text.strip (Text.pack (firstLine (Char8.unpack err))))
  where
    firstLine said = case lines said of
      (line : _) -> line
      [] -> "the door refused without saying why"
    -- "chain verified: 4 line(s), tail seq 3"
    tailSeq said = case reverse (words said) of
      (final : _) -> maybe 0 fst (listToMaybeRead (reads final))
      [] -> 0
    listToMaybeRead = \case
      (parsed : _) -> Just parsed
      [] -> Nothing

-- | The JSON one verb travels as.
--
-- Encoded through aeson rather than by pasting strings together: a frame this
-- adversary built by hand and got subtly wrong would be reported as a refusal by
-- the city, and a refusal that came from a typo here is indistinguishable from
-- one the product owed.
frameOf :: Verb -> Text
frameOf = \case
  CityView -> render (object ["query" .= ("city_view" :: Text)])
  EndpointView -> render (object ["query" .= ("endpoint_view" :: Text)])
  CreateBuilding addr template (IdemKey idem) ->
    command
      "create_building"
      [ "addr" .= addr
      , "template" .= templateName template
      , "idem" .= idem
      ]
  Dispatch addr session (IdemKey idem) ->
    command
      "dispatch"
      [ "addr" .= addr
      , "task" .= ("say something" :: Text)
      , "goal" .= ("an answer" :: Text)
      , "mode" .= ("build" :: Text)
      , "budget" .= object ["usd" .= (1000 :: Word64), "tokens" .= (1000 :: Word64)]
      , "idem" .= idem
      , "session" .= session
      , "effort" .= Aeson.Null
      ]
  Halt scope (IdemKey idem) -> command "halt" ["scope" .= scopeValue scope, "idem" .= idem]
  Release scope (IdemKey idem) -> command "release" ["scope" .= scopeValue scope, "idem" .= idem]
  Takeover run (IdemKey idem) -> command "takeover" ["run" .= run, "idem" .= idem]
  Verbatim raw -> raw
  where
    command name fields = render (object ["command" .= object [name .= object fields]])
    render = Text.pack . Char8.unpack . Aeson.encode

templateName :: Template -> Text
templateName Minimal = "minimal"
templateName Confidential = "confidential"

-- | A scope is a bare word or a one-field object, which is how serde renders
-- an enum whose variants differ in whether they carry anything.
scopeValue :: Scope -> Value
scopeValue City = Aeson.String "city"
scopeValue (Building addr) = object ["building" .= addr]

-- | Runs the program and takes both streams as bytes.
--
-- Bytes rather than text: recovery lines carry punctuation outside ASCII, and
-- a locale-decoded stream would corrupt them on the way in and then fail to
-- parse for a reason that has nothing to do with the product.
capture :: FilePath -> [String] -> IO (ExitCode, Lazy.ByteString, Lazy.ByteString)
capture binary arguments = do
  attempted <- try run
  case attempted of
    Right outcome -> pure outcome
    Left err ->
      fail ("the binary could not be run: " <> show (err :: IOException))
  where
    run =
      withCreateProcess
        (proc binary arguments) {std_in = NoStream, std_out = CreatePipe, std_err = CreatePipe}
        $ \_ out err handle ->
          case (out, err) of
            (Just outHandle, Just errHandle) -> do
              hSetBinaryMode outHandle True
              hSetBinaryMode errHandle True
              reported <- Lazy.hGetContents outHandle
              complained <- Lazy.hGetContents errHandle
              -- Forced before waiting rather than after: a pipe left unread
              -- keeps the child alive, so waiting first would deadlock the
              -- pair on any output larger than one buffer.
              _ <- evaluate (Lazy.length reported)
              _ <- evaluate (Lazy.length complained)
              status <- waitForProcess handle
              pure (status, reported, complained)
            _ -> fail "the child process was created without the pipes it was asked for"
