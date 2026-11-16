# Kneekura Prompt Wheeler India

用途別のAIプロンプトを **ドーナツ型ホイールで選び、1クリックでコピー** できる、依存ライブラリなしの静的Webアプリです。

## 収録・操作

- **45件 / 8ジャンル**：開発12、調査5、学習6、文章6、創作4、デザイン4、仕事4、生活4
- ホイール下スクロールで円環が**時計回り（右回転）**し、次のジャンルに移動
- ジャンルのクリック、前後ボタン、左右矢印キーも対応
- 本文ワンクリックコピー、全ジャンル検索（Ctrl/Cmd + K）、ブラウザ内のお気に入り保存
- スマホのタップ操作とOSの動きを減らす設定にも対応
- ユーザー提供のVoxプロンプト6件は英語原文と日本語訳の両方を収録

## 実行方法

ビルド不要です。リポジトリ内で `python -m http.server 8000` を実行し、ブラウザで `http://localhost:8000` を開きます。`index.html` の直接ファイル実行では、ブラウザの制限によりJSONを読み込めない場合があります。

GitHub Pagesの場合は、所有者が Settings → Pages から公開ブランチと `/ (root)` を指定してください。**このPRでは公開設定を変更していません。**

## Windows デスクトップ版（Tauri 2 / WebView2）

Web版をそのままブラウザで使えることは維持しつつ、Windowsの常駐ウィンドウ版を `src-tauri/` に用意しています。メインのプロンプトデータはWeb版と共通です。

- **起動直後は約430×675のコンパクト表示**。ウィンドウの境界をドラッグして手動リサイズ可能。コンパクト／一覧のボタンはそれぞれ最後に設定したサイズを保存します。
- **最前面固定は初期状態でON**。右上「固定中」ボタンで自由に解除・再設定できます。
- **外側の円環にマウスを乗せてホイール** → ジャンルが時計回りに切替。
- **円の中央にマウスを乗せてホイール** → 同じジャンル内の個別プロンプトを切替。中央の「コピー」で本文を取得できます。
- 画面内の「隠す」またはWindowsの通常の閉じるボタンで**タスクトレイへ収納**。トレイアイコンの左クリックで表示／非表示、右クリックで「表示・隠す・終了」を選択できます。
- ネットワーク接続や外部AIサービスへの送信は不要。プロンプトは起動ファイルに同梱します。お気に入りとコンパクト・最前面の設定はローカルに保存します。
- フロントエンドの依存フレームワークを増やさず、システムのWebView2を再利用する方式です。**常駐メモリ量はまだ実測していないので、数値は保証しません**。

### 開発実行（Windows 10/11）

必要環境：Node.js、Rust、Microsoft C++ Build Tools、WebView2 Runtime。詳細は [Tauri Windows前提条件](https://v2.tauri.app/start/prerequisites/) を参照してください。

```powershell
npm install
npm run desktop:check
npm run desktop:dev
```

通常インストーラーを作る場合：

```powershell
npm run desktop:build
```

WindowsのNSISインストーラーは `src-tauri/target/release/bundle/nsis/` に作られます。PRのGitHub Actions `Windows Desktop Installer` が成功した場合は、実行結果の Artifacts からビルド成果物を取得できます（未検証ビルドを完成品とみなさないでください）。

`scripts/prepare-desktop.mjs` はWeb版の必要な静的ファイルのみ `desktop-dist/` へコピーし、`scripts/generate-icons.mjs` でアプリ専用のPNG/ICOを作ります。生成物はGitに登録しません。Python、React、Electronを実行時に同梱する必要はありません。

### 動作確認の範囲

JSONデータ・JavaScript構文・静的アセット・Tauri設定・画像ヘッダを自動チェックします。Windowsでの実際のインストール、タスクトレイ、ホイール感度、マルチモニター、アイコン表示、画面操作は別途実機検証が必要です。

## データと出典

`data/prompts.json` が正本です。各項目に `id`, `title`, `category`, `summary`, `prompt`, `source` を持ち、画像由来の6件には `promptEn` も付けています。

`source.type` は次のいずれかです。

- `user-supplied`：ユーザー提供画像のVox (@Voxyz_ai) 6件。原文と日本語訳を保存。**個別投稿URLは未確認**のため作者プロフィールへリンクします。
- `adapted`：Redditの実例から発想や手順を参考に、**独自の日本語プロンプトとして再構成**したもの。元投稿の全文転載ではありません。
- `original`：本プロジェクトのため新規に考案したもの。

参考元には r/ClaudeAI、r/ClaudeCode、r/ChatGPTCoding、r/PromptEngineering、r/Learning、r/ChatGPTPromptGenius、r/AIWritingLounge などがあります。元投稿のリンクは各レコードとカードに保存しています。

**出典を記すだけでは転載許諾になりません。** 外部の文章の権利はそれぞれの権利者に帰属します。今後収集するときも、独自再構成・明示ライセンス付きの複製・ユーザー提供を必ず区別してください。外部候補の [f/prompts.chat](https://github.com/f/prompts.chat) はプロンプトデータにCC0を採用していますが、本試作へは一括取込していません。

プロンプトの効果は未検証です。特に低頻度バグを放置する指示は、重大なセキュリティ・安全・データ損失のケースへそのまま適用しないでください。外部ツールを使うよう要求するプロンプトは、対象AIにそのツールがあるときだけ実行可能です。

## 追加・検証

`data/prompts.json` の `prompts` に、固有のID、用途の明確なタイトル、説明、日本語本文、参照元とその種別を追加します。

```bash
node --check app.js
node scripts/validate-catalog.mjs
```

変更時の基本検証用に `.github/workflows/validate.yml` を用意しています。まだ各プロンプトの実際の回答品質まではテストしていません。

## 今後の開発案

検索タグの細分化、【変数】入力フォーム、プロンプトのユーザー登録とインポート、参考元更新履歴、テーマ切り替えなどは将来の候補です。現時点の試作には、Redditの自動収集や外部AIへのプロンプト送信機能はありません。
