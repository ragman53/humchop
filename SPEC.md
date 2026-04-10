# HumChop - SPEC.md

## 1. プロジェクト概要

**タイトル**
HumChop - 鼻歌で即座にサンプルをチョップ＆割り当てるサンプリング作曲ツール

**コンセプト**
ユーザーはサンプル音源（ドラムループ、ボーカル、メロディなど）を聴きながら「この感じでチョップしたい！」というアイデアを鼻歌（ハミング）で即興表現する。
アプリは鼻歌を解析し、サンプルを自動でチョップして各ノートに適切に割り当て、**新しいチョップサンプル**を即生成する。
→ 「聴く → 鼻歌で考える → 即完成」という直感的なサンプリング作曲フローを実現。

**ターゲットユーザー**
- サンプリングを多用するビートメイカー / トラックメイカー
- アイデアを素早く形にしたいDTMユーザー
- 鼻歌やハミングで作曲する習慣がある人

**MVP目標**
TUIで動作する最小動作版（鼻歌録音 → 解析 → チョップ → 割り当て → WAV出力）をRustで実装。
1週間以内にプロトタイプ完成を想定。GUIはPost-MVP（Phase 2以降）とする。

---

## 2. 機能要件（Functional Requirements）

### 2.1 必須機能（MVP / TUI）

**1. サンプル音源ロード**
- WAV / MP3 / FLAC 対応
  - WAV読み書き：`hound`
  - MP3 / FLAC デコード：`symphonia`（エンコードはWAVのみ対応。MVP出力はWAV固定）
- CLIではファイルパス指定

**2. サンプルプレビュー再生**
- ロードしたサンプルをTUI上でプレビュー再生（`rodio`）
- rodioはsymphoniaのfeatureを有効化してMP3 / FLACをデコード

**3. 鼻歌録音**
- マイク入力で最大15秒録音（`cpal` with `pulseaudio` feature）
- 録音開始 / 停止はTUIキーバインド（`r` キーでトグル）
- 録音バッファは `tokio::sync::mpsc` チャンネル経由でTUIスレッドに渡す

**4. 鼻歌解析（Hum Transcription）**
- ピッチ検出：YIN / McLeod アルゴリズム（`pitch-detection` クレート）
  - モノフォニック前提。背景ノイズには弱いため、静音環境を推奨
  - 精度が要件（後述）を満たせない場合、Phase 2でBasic Pitch（ONNX）に移行する
- オンセット / オフセット検出：スペクトルフラックス（`rustfft` + `dasp`）
- ノートシーケンス生成：`Vec<Note>` として保持

```rust
struct Note {
    pitch_hz: f32,
    onset_sec: f64,
    duration_sec: f64,
    velocity: f32,
}
```

- 精度要件：10回の鼻歌テストで7回以上（70%以上）、意図したノート数と±1以内に収まること。未達の場合はBasic Pitch移行を検討する

**5. サンプル自動チョップ**
- 鼻歌のノート数に合わせてサンプルを分割
- 分割方式（2種、TUIで選択可能）：
  - 均等分割（デフォルト）：鼻歌の合計長をノート数で等分
  - オンセット検出分割：サンプル内のオンセット点を `rustfft` で検出し、自然なチョップポイントを探索
- 各チョップをWAVスライス（`Vec<f32>`）として保持

**6. チョップ割り当て（Mapping）**
- 各鼻歌ノートに最も近いチョップをマッピング（ピッチ距離最小）
- タイムストレッチ：`rubato`（タイムストレッチ専用。ピッチシフトとは別処理）
- ピッチシフト：`rubato` によるタイムストレッチ + `dasp` による再サンプリングの組み合わせで実現
  - 注意：`rubato` はタイムストレッチ専用クレートであり、ピッチシフト単体の機能は持たない
- ベロシティによるゲイン調整
- MVP簡略化オプション：ピッチシフトなし（均等ゲイン割り当てのみ）での動作も可とする

**7. 出力**
- チョップ済みWAVファイル生成（`output_chopped_{timestamp}.wav`）
- TUI上に「完成！」メッセージ + 再生コマンド表示

**8. TUIインターフェース**
- フレームワーク：`ratatui` + `crossterm`（event-stream feature）
- 非同期ランタイム：`tokio`
- イベントループ：`tokio::select!` で以下を多重化
  - キーイベント（crossterm EventStream）
  - Tick（定期状態更新）
  - Render（フレームレート制御）
  - AudioBuffer（cpal → mpsc → TUI）
- 画面構成（案）：
  - ヘッダー：タイトル / 操作ガイド
  - メインエリア：波形テキスト表示 / ノートシーケンス表示
  - フッター：ステータスバー / 録音レベルメーター

### 2.2 エラーケース（明示的に処理すること）

| エラー | 対応 |
|--------|------|
| マイクデバイスが見つからない | わかりやすいメッセージ + 設定確認コマンドを表示 |
| サンプルの長さ < 鼻歌のノート数 | 均等分割にフォールバック + 警告表示 |
| ノートが1個しか検出されない | ユーザーに再録音を促す |
| 対応外フォーマット（OGGなど） | 非対応フォーマット旨を明示 |
| WSL2環境でPULSE_SERVERが未設定 | 設定手順をコンソールに案内 |

### 2.3 将来拡張機能（Phase 2以降 / Post-MVP）

- **GUI**：Dioxus 0.7（TUIコードと `core` クレートを共有するWorkspace構成）
- Basic Pitch（ONNX）による高精度ピッチ検出（`tract-onnx`）
- MIDI出力（鼻歌をMIDIノートとしてエクスポート）
- SFZ / Sampler Patch出力
- ドラム専用モード（stem separation統合）
- 複数サンプル同時ロード＆レイヤリング
- Web版（Dioxus WASM + Axum on Leapcell）

---

## 3. 非機能要件（Non-Functional Requirements）

- **言語**：Rust（安定版、MSRV 1.75以上）
- **パフォーマンス**：鼻歌解析＋チョップを10秒以内で完了（MVP）
- **クロスプラットフォーム**：macOS / Windows / Linux（WSL2含む）対応
- **依存ライブラリ**：最小限（Cargo.tomlで明記、後述）
- **ライセンス**：Apache 2.0 または MIT
- **エラー処理**：`anyhow` + `colored` によるユーザーフレンドリーなメッセージ
- **ログ**：`env_logger`（`RUST_LOG=debug` で詳細出力）

---

## 4. WSL2環境セットアップ

WSL2では直接サウンドカードドライバが存在しないため、WSLg（Windows 11同梱）のPulseAudio RDPブリッジを利用する。

**動作条件**
- Windows 11（WSLg同梱）
- WSL2 カーネル 5.15 以上
- Windowsの「設定 → プライバシーとセキュリティ → マイク」でターミナルアプリへのアクセスを許可

**セットアップ手順**

```bash
# 依存パッケージ（Debian / Ubuntu）
sudo apt update
sudo apt install libasound2-dev libpulse-dev libasound2-plugins alsa-utils pulseaudio-utils

# ALSAをPulse経由に向ける
cat >> ~/.asoundrc << 'EOF'
pcm.!default { type pulse }
ctl.!default { type pulse }
EOF

# PulseServerソケットを環境変数に設定（~/.bashrc または ~/.zshrc に追記）
echo 'export PULSE_SERVER=unix:/mnt/wslg/PulseServer' >> ~/.bashrc
source ~/.bashrc

# 動作確認
pactl list sources short   # RDPSource が表示されればOK
arecord -D default -f S16_LE -r 44100 -c 1 -d 1 /tmp/test.wav && echo "録音OK"
aplay /tmp/test.wav && echo "再生OK"
```

**cpalでの注意事項**

WSL2では `cpal` の ALSA バックエンド（デフォルト）は PulseAudio が排他的にデバイスを保持するため `DeviceBusy` エラーになる。必ず `pulseaudio` featureを有効にすること。

```toml
cpal = { version = "0.16", features = ["pulseaudio"] }
```

**既知の問題**

Ubuntu 24.04のWSL2で `systemd` による `pulseaudio.service` の自動起動に失敗するケースがある。
`PULSE_SERVER=unix:/mnt/wslg/PulseServer` を直接環境変数で指定することで回避できる。

---

## 5. システムアーキテクチャ

```
[TUI Frontend (ratatui + crossterm)]
         ↓  tokio::select! (KeyEvent / Tick / Render / AudioBuffer)
[Main Loop (tokio async)]
├── Sample Loader      (hound + symphonia)
├── Audio Preview      (rodio)
├── Hum Recorder       (cpal --features pulseaudio)
│     └── mpsc channel → TUI
├── Hum Analyzer       (pitch-detection + rustfft + dasp)
│     └── Note: Vec<(pitch_hz, onset_sec, duration_sec, velocity)>
├── Sample Chopper     (均等分割 or オンセット検出)
├── Mapper             (rubato タイムストレッチ + dasp 再サンプリング)
└── Output Writer      (hound → output_chopped_{timestamp}.wav)
```

**モジュール構成**（src/ 配下）

```
src/
├── main.rs          - エントリポイント・tokio runtime起動
├── tui.rs           - ratatui TUI構造体・イベントループ
├── hum_analyzer.rs  - ピッチ検出・オンセット検出・Note生成
├── sample_chopper.rs- チョップロジック（均等 / オンセット）
├── mapper.rs        - チョップ割り当て・タイムストレッチ・ピッチシフト
├── audio_utils.rs   - 共通オーディオ処理（サンプル型変換・正規化）
└── error.rs         - アプリ固有エラー型
```

**Post-MVP Workspace構成（参考）**

```
humchop/
├── Cargo.toml          # workspace
└── crates/
    ├── core/           # humchop-core（lib）← MVPのロジックをここに移植
    ├── tui/            # humchop-tui（bin）← MVPのTUI
    └── server/         # humchop-server（bin）← Phase 2 Axum
```

MVPではWorkspace化は不要だが、`hum_analyzer` / `sample_chopper` / `mapper` / `audio_utils` はPost-MVPでの `core` 切り出しを想定し、`cpal` / `ratatui` への直接依存を持たない純粋なロジックとして設計すること。

---

## 6. 技術スタック（MVP TUI）

| カテゴリ | ライブラリ | バージョン | 用途 |
|----------|-----------|-----------|------|
| TUI | ratatui | 0.29 | ターミナルUI描画 |
| ターミナルバックエンド | crossterm | 0.28 | キーイベント・画面制御 |
| 非同期ランタイム | tokio | 1 | イベントループ多重化 |
| オーディオI/O | cpal | 0.16 | 録音（pulseaudio feature必須 on WSL2） |
| WAV読み書き | hound | 3.5 | WAVのみ。出力フォーマット |
| デコード | symphonia | 0.5 | MP3 / FLAC / WAV デコード |
| オーディオ再生 | rodio | 0.19 | サンプルプレビュー再生 |
| DSP共通 | dasp | 0.11 | サンプル型変換・再サンプリング |
| ピッチ検出 | pitch-detection | 0.3 | YIN / McLeod（モノフォニック） |
| FFT | rustfft | 6.2 | スペクトルフラックス・オンセット検出 |
| タイムストレッチ | rubato | 0.15 | タイムストレッチ（ピッチシフトは dasp 再サンプリングと組み合わせ） |
| エラー処理 | anyhow | 1 | - |
| ログ | env_logger | 0.11 | RUST_LOG=debug で詳細出力 |
| カラー出力 | colored | 2 | ユーザーフレンドリーなメッセージ |

**Cargo.toml（MVP）**

```toml
[dependencies]
# TUI
ratatui = "0.29"
crossterm = { version = "0.28", features = ["event-stream"] }
tokio = { version = "1", features = ["full"] }
futures = "0.3"

# Audio I/O
cpal = { version = "0.16", features = ["pulseaudio"] }
hound = "3.5"
symphonia = { version = "0.5", features = ["mp3", "flac", "wav"] }
rodio = { version = "0.19", default-features = false, features = ["symphonia-mp3", "symphonia-flac"] }

# DSP
rubato = "0.15"
rustfft = "6.2"
dasp = { version = "0.11", features = ["signal", "interpolate"] }
pitch-detection = "0.3"

# エラー・ログ
anyhow = "1"
env_logger = "0.11"
colored = "2"
```

---

## 7. データフロー

```
1. ユーザー → サンプルファイルパス入力 (TUI)
2. Sample Loader → symphonia でデコード → Vec<f32> バッファ
3. ユーザー → 鼻歌録音開始 (TUI キーバインド)
4. cpal → PCM コールバック → mpsc チャンネル → Vec<f32> バッファ
5. 鼻歌 Vec<f32> → Hum Analyzer → Vec<Note>
6. サンプル Vec<f32> + Vec<Note> → Sample Chopper → Vec<Vec<f32>> (チョップスライス)
7. Vec<Note> + Vec<Vec<f32>> → Mapper → 最終 Vec<f32>
8. 最終 Vec<f32> → hound → output_chopped_{timestamp}.wav
```

---

## 8. MVP開発マイルストーン

| Day | タスク | 完了条件 |
|-----|--------|---------|
| Day 1 | プロジェクト作成・サンプルロード・WAV出力 | `cargo run sample.wav` でWAVを読み込み、同一内容を書き出せる |
| Day 2 | ratatui TUI基盤・イベントループ | TUIが起動し、`q` で終了できる |
| Day 3 | cpalマイク録音 → mpsc → TUI表示 | `r` キーで録音開始/停止、録音バッファをTUIに表示できる |
| Day 4 | ピッチ検出・オンセット検出 | テスト用WAVから `Vec<Note>` を生成し、コンソール出力できる |
| Day 5 | 均等分割チョップ + ゲイン割り当てのみ（ピッチシフトなし） | チョップ済みWAVを出力できる（音質は問わない） |
| Day 6 | タイムストレッチ統合 + 統合テスト | 実際の鼻歌で一通りのフローが動作する |
| Day 7 | エラーハンドリング・WSL2動作確認・ドキュメント | READMEにセットアップ手順を記載、WSL2で全機能動作確認 |

**Day 5-6 注意：** ピッチシフトは実装コストが高い。Day 5 は「均等ゲイン割り当てのみ」でWAV出力を最優先とし、ピッチシフトはDay 6以降または Phase 1.5 に移してよい。

---

## 9. 制約・注意事項

- 初回はドラムループやモノフォニックメロディサンプルを想定（ポリフォニック対応はPhase 2）
- `rubato` はタイムストレッチ専用。ピッチシフトは `dasp` 再サンプリングとの組み合わせが必要
- `symphonia` はデコード専用。MP3 / FLAC のエンコード機能はなく、出力はWAVのみ（`hound`）
- WSL2環境では `cpal` の `pulseaudio` feature が必須（ALSA バックエンドは DeviceBusy になる）
- 鼻歌精度はユーザーの歌い方と環境ノイズに依存。フィードバックUIはPost-MVPで追加
- 商用利用可のOSSのみ使用（Apache 2.0 / MIT / BSD 系のみ）

---

## 10. 承認・次アクション

このSPECで問題なければ「OK」または「ここを修正」と返信してください。
修正後、即座に `Cargo.toml` + 完全動作するMVPコード一式を出力します。
