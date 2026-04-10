# HumChop - Code Review Notes

## 2. Rust Coding Principles 遵守状況

### 良い点

- モジュール分割が明確（concern separation ができている）。
- `anyhow` + 自前 `HumChopError` の組み合わせが上手（`Display` が親切）。
- `#[derive(Debug, Clone, PartialEq)]` や `Default` の多用が Rust らしい。
- `feature = "audio-io"` で cpal/rodio を optional にしているのは正解。
- `ratatui` + `tokio` + `crossterm` の組み合わせも最新のベストプラクティスに沿っています。
- `sample_chopper.rs` v0.3.0: マルチバンド検出、MAD正規化、ピーク prominenc e — 非常に高品質なアルゴリズム実装。

### 改善済み（v0.3.0）

- ✅ `sample_chopper.rs` のトランジェント検出が大幅改善（pre-emphasis、マルチバンド、ピークピッキング）
- ✅ 不要な `#[allow(dead_code)]` を一部解消（`DillaConfig` の未使用フィールドに明示的に付与）
- ✅ 全テストパス（40件）

### 改善すべき点（優先順）

1. **残りの `clippy` warning 解消** — `mapper.rs` の `apply_fade()` ループ、`tui.rs` の `option_as_ref_deref` など。機能に影響しないがコード品質向上のため対応すべき。
2. **エラー処理の一貫性** — ところどころ `anyhow::anyhow!` と `HumChopError` が混在。可能なら `HumChopError` で統一するか `From<anyhow::Error>` を実装。
3. **パフォーマンス / 所有権**
   - `tui.rs` の `process_hum` で `self.sample.clone()` している（巨大な `Vec<f32>` を clone）。参照または `Arc` に変更すべき。
   - `mapper.rs` の `estimate_chop_pitch` で毎回 `HumAnalyzer::new` を作っている。キャッシュすべき。
   - `mapper.rs` の `linear_resample` は自前実装。`rubato`（依存済み）の `SincResampler` を使う方が高速・高品質。
4. **f32 の partial_cmp** — 複数箇所で `unwrap_or(Ordering::Equal)` している。Rust 1.62+ の `f32::total_cmp` を検討。

---

## 3. 具体的なコードごとのコメント（主要モジュール）

| モジュール | 評価 | コメント |
|-----------|------|----------|
| `error.rs` | ✅  Excellent | ユーザー向けメッセージが丁寧。`From` 実装も網羅的。 |
| `audio_utils.rs` | ✅  Good | `symphonia` の使い方もほぼ正しい。未使用関数 `create_test_wav`、`create_named_temp_wav` がテスト内にある。 |
| `hum_analyzer.rs` | ✅  Good | ロジックは堅実。`YINDetector` を毎フレーム新規作成しているが、再利用可能にすれば若干高速化できる。 |
| `sample_chopper.rs` | ✅  Excellent (v0.3.0) | マルチバンド検出、MAD正規化、ピークピッキング — プロダクション品質。FFT planner のキャッシュを検討すれば更に高速化可能。 |
| `mapper.rs` | ⚠️  Good | ロジックは正しい。`linear_resample` → `rubato` 置換、`apply_fade` の clippy warning 対応が残り。 |
| `recorder.rs` | ✅  Good | 正規化（i16/U16 → f32）が正確。WSL2 対応も丁寧。 |
| `player.rs` | ✅  Good | 必要十分な実装。 |
| `tui.rs` | ✅  Good | 状態管理（`AppState`）がしっかりしている。残りの clippy warning 2件が未対応。 |
| `main.rs` | ⚠️  Good | CLI パーサーは完璧。`run_interactive` が長い（~150行）。関数分割を検討。clippy warning 3件が残り。 |

---

## 4. まとめ & 次のアクション提案

**現在の完成度**: 85%（コアロジックは高品質、polish が残り少し）

**最優先でやるべきこと（v0.3.1）**:

1. クロスフェード間の chop（現在 5ms ギャップ → スムーズなクロスフェード）
2. `mapper.rs` の `linear_resample` → `rubato::SincResampler` 置換
3. `--no-tui` ヘッドレス CLI モード追加
4. 残りの clippy warning 解消
