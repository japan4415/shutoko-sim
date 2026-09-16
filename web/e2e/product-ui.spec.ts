// issue #15 完了条件の E2E 検証。
// (1) 住所検索 → 候補選択 → 探索 → カード / Maps URL
// (2) 現在地取得成功
// (3) 現在地取得拒否（再要求しない）
// (4) 住所検索失敗（429 / RATE_LIMITED。入力保持）
// (5) 候補なしからの時間拡大復帰
// (6) 地図・カード選択同期と帰属表示 / 経路線
// (7) モバイル 375px で横 overflow なし
// (8) 色に依存しない候補番号バッジ
import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";

const GEOCODE_URL = "**/api/geocode";

/** 住所検索の候補（外部ジオコーダへは出さず route スタブで返す）。 */
const CANDIDATES = [
  { label: "東京都千代田区大手町", lat: 35.6866, lon: 139.7643 },
  { label: "東京都中央区日本橋", lat: 35.6836, lon: 139.7742 },
];

/**
 * アプリを開く。座標プリセットは折りたたみ内にあるため、テストからは開いて操作する
 * （vertical-slice.spec.ts と同じ規約）。
 */
async function openApp(page: Page): Promise<void> {
  await page.goto("/");
  await expect(page.locator("#search-btn")).toBeVisible();
  await page.locator("#manual-input").evaluate((node) => {
    (node as HTMLDetailsElement).open = true;
  });
}

/** 住所検索をスタブする。/api/geocode へ出る前に route で応答する。 */
async function stubGeocode(
  page: Page,
  candidates: unknown,
  status = 200,
): Promise<void> {
  await page.route(GEOCODE_URL, async (route) => {
    await route.fulfill({
      status,
      contentType: "application/json",
      body: JSON.stringify(candidates),
    });
  });
}

/** getCurrentPosition を決定論的にスタブする。呼び出し回数は __geoCallCount で数える。 */
async function stubGeolocationSuccess(
  page: Page,
  coords: { lat: number; lon: number },
): Promise<void> {
  await page.addInitScript(
    ({ lat, lon }) => {
      const w = window as unknown as { __geoCallCount: number };
      w.__geoCallCount = 0;
      Object.defineProperty(navigator, "geolocation", {
        configurable: true,
        value: {
          getCurrentPosition: (
            onSuccess: (p: unknown) => void,
            _onError?: (e: unknown) => void,
          ) => {
            w.__geoCallCount += 1;
            onSuccess({
              coords: { latitude: lat, longitude: lon, accuracy: 5 },
              timestamp: Date.now(),
            });
          },
        },
      });
    },
    coords,
  );
}

/** getCurrentPosition が失敗コールバック（code）を呼ぶようスタブする。 */
async function stubGeolocationFailure(page: Page, code: number): Promise<void> {
  await page.addInitScript(
    ({ errorCode }) => {
      const w = window as unknown as { __geoCallCount: number };
      w.__geoCallCount = 0;
      Object.defineProperty(navigator, "geolocation", {
        configurable: true,
        value: {
          getCurrentPosition: (
            _onSuccess: (p: unknown) => void,
            onError?: (e: unknown) => void,
          ) => {
            w.__geoCallCount += 1;
            onError?.({ code: errorCode, message: "denied", PERMISSION_DENIED: 1 });
          },
        },
      });
    },
    { errorCode: code },
  );
}

/** 住所検索を確定させて出発地点にする。 */
async function searchByAddress(page: Page, index = 0): Promise<string> {
  const label = CANDIDATES[index]?.label ?? "";
  await page.fill("#address-query", label);
  await page.click("#address-search-btn");
  await expect(
    page.locator("#address-candidates input[type=radio]"),
  ).toHaveCount(CANDIDATES.length);
  await page.locator("#address-candidates input[type=radio]").nth(index).check();
  await expect(page.locator("#origin-summary")).toContainText(label);
  return label;
}

/** 時間範囲を設定する。 */
async function setTimeRange(page: Page, min: string, max: string): Promise<void> {
  await page.fill("#min-minutes", min);
  await page.fill("#max-minutes", max);
}

// 各テストの後に route ハンドラを確実に破棄する（pending は ignoreErrors で待たない）。
test.afterEach(async ({ page }) => {
  productDelayAbort?.abort();
  productDelayAbort = null;
  await page.unrouteAll({ behavior: "ignoreErrors" });
});

/** キャンセルテストの graph.json 遅延をテスト終了時に解除するためのコントローラ。 */
let productDelayAbort: AbortController | null = null;

test("(1) 住所検索の候補選択で出発地点が確定し探索できる", async ({ page }) => {
  await stubGeocode(page, { candidates: CANDIDATES });
  await openApp(page);

  await page.fill("#address-query", "大手町");
  await page.click("#address-search-btn");

  // 候補は radio として 2 件表示される。
  const radios = page.locator("#address-candidates input[type=radio]");
  await expect(radios).toHaveCount(2);

  // 候補を選ぶと初めて出発地点が当該住所へ確定する（プリセットから変わる）。
  await expect(page.locator("#origin-summary")).not.toContainText("東京都中央区日本橋");

  await radios.nth(1).check();
  await expect(page.locator("#origin-summary")).toContainText("東京都中央区日本橋");

  await setTimeRange(page, "15", "60");
  await page.click("#search-btn");

  const firstCard = page.locator("#results .card").first();
  await expect(firstCard).toBeVisible();
  // 入口・出口名と Maps URL。
  await expect(firstCard).toContainText("入口");
  await expect(firstCard).toContainText("出口");
  const mapsUrl = await firstCard.getAttribute("data-maps-url");
  expect(mapsUrl, "data-maps-url 属性").not.toBeNull();
  expect(mapsUrl?.startsWith("https://www.google.com/maps/dir/?api=1")).toBe(true);
});

test("(2) 現在地取得成功で出発地点に（現在地）と表示され探索できる", async ({ page }) => {
  await stubGeolocationSuccess(page, { lat: 35.6896727, lon: 139.7644248 });
  await stubGeocode(page, { candidates: CANDIDATES });
  await openApp(page);

  await page.click("#geolocate-btn");
  await expect(page.locator("#origin-summary")).toContainText("（現在地）");

  await setTimeRange(page, "15", "60");
  await page.click("#search-btn");
  await expect(page.locator("#results .card").first()).toBeVisible();
});

test("(3) 現在地取得拒否で権限メッセージと住所検索導線が出て再要求しない", async ({ page }) => {
  await stubGeolocationFailure(page, 1);
  await openApp(page);

  await page.click("#geolocate-btn");

  await expect(page.locator("#address-feedback")).toContainText(
    "位置情報の利用が許可されていません",
  );
  await expect(page.locator("#recovery-actions")).toBeVisible();
  await expect(
    page.locator("#recovery-actions button", { hasText: "住所検索を使う" }),
  ).toBeVisible();

  // 拒否後に権限を再要求していない。
  const calls = await page.evaluate(
    () => (window as unknown as { __geoCallCount: number }).__geoCallCount,
  );
  expect(calls).toBe(1);
});

test("(4) 住所検索失敗は RATE_LIMITED 文言を出し時間入力を保持する", async ({ page }) => {
  await stubGeocode(page, { error: { code: "RATE_LIMITED", retryable: true } }, 429);
  await openApp(page);
  await setTimeRange(page, "30", "90");

  await page.fill("#address-query", "大手町");
  await page.click("#address-search-btn");

  await expect(page.locator("#address-feedback")).toContainText(
    "検索回数の上限に達しました",
  );
  // 時間入力は保持される。
  await expect(page.locator("#min-minutes")).toHaveValue("30");
  await expect(page.locator("#max-minutes")).toHaveValue("90");
});

test("(5) 候補なしから時間を広げる導線で上限が増える", async ({ page }) => {
  await stubGeocode(page, { candidates: CANDIDATES });
  await openApp(page);
  await searchByAddress(page, 0);

  // 60〜90 は既存テストと同じく候補なしになる窓。
  await setTimeRange(page, "60", "90");
  await page.click("#search-btn");

  await expect(page.locator("#status")).toContainText("候補がありません");
  const recovery = page.locator("#recovery-actions");
  await expect(recovery).toBeVisible();
  const widen = recovery.locator("button", { hasText: "時間の上限を広げる" });
  await expect(widen).toBeVisible();

  await widen.click();
  // 90 → 120 に増え、フォーカスも移る。
  await expect(page.locator("#max-minutes")).toHaveValue("120");
});

test("(6) カード選択と地図が同期し帰属表示と経路線がある", async ({ page }) => {
  await stubGeocode(page, { candidates: CANDIDATES });
  await openApp(page);
  await searchByAddress(page, 0);
  await setTimeRange(page, "15", "60");
  await page.click("#search-btn");

  const cards = page.locator("#results .card");
  await expect(cards).toHaveCount(2);
  // 先頭候補が初期選択される。
  await expect(cards.nth(0)).toHaveAttribute("aria-current", "true");
  await expect(cards.nth(1)).toHaveAttribute("aria-current", "false");

  await cards.nth(1).click();
  await expect(cards.nth(1)).toHaveAttribute("aria-current", "true");
  await expect(cards.nth(0)).toHaveAttribute("aria-current", "false");

  // 帰属は国土地理院・OpenStreetMap の両方を常時表示する。
  const attribution = page.locator("#map .leaflet-control-attribution");
  await expect(attribution).toContainText("OpenStreetMap");
  await expect(attribution).toContainText("国土地理院");

  // 経路の線が描画されている。
  await expect(page.locator("#map .leaflet-overlay-pane path").first()).toBeAttached();
});

test("(7) モバイル 375px で横 overflow せず主要操作が可視", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await stubGeocode(page, { candidates: CANDIDATES });
  await openApp(page);
  await searchByAddress(page, 0);
  await setTimeRange(page, "15", "60");
  await page.click("#search-btn");
  await expect(page.locator("#results .card").first()).toBeVisible();

  const overflow = await page.evaluate(() => {
    const el = document.scrollingElement ?? document.documentElement;
    return el.scrollWidth <= window.innerWidth + 1;
  });
  expect(overflow, "横方向に overflow しない").toBe(true);

  await expect(page.locator("#address-search-btn")).toBeVisible();
  await expect(page.locator("#geolocate-btn")).toBeVisible();
  await expect(page.locator("#search-btn")).toBeVisible();
});

test("(8) 各カードに 1 始まりの候補番号バッジがある", async ({ page }) => {
  await stubGeocode(page, { candidates: CANDIDATES });
  await openApp(page);
  await searchByAddress(page, 0);
  await setTimeRange(page, "15", "60");
  await page.click("#search-btn");

  const cards = page.locator("#results .card");
  await expect(cards).toHaveCount(2);
  const badges = page.locator("#results .card .candidate-index");
  await expect(badges).toHaveCount(2);
  await expect(badges.nth(0)).toHaveText("1");
  await expect(badges.nth(1)).toHaveText("2");
});

// --- issue #15 追加の must-have テスト（レビュー指摘の未カバー経路） ---

const PRODUCT_GRAPH_URL = "**/releases/c1-real-v1/graph.json";

/** abort 可能な待機。abort されたら即座に resolve する（vertical-slice と同じ規約）。 */
function delayUnlessAbortedProduct(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    function onAbort(): void {
      clearTimeout(timer);
      resolve();
    }
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

/** 探索までの共通準備（住所確定 → 15〜60 設定）。 */
async function prepareSearch(page: Page): Promise<void> {
  await stubGeocode(page, { candidates: CANDIDATES });
  await openApp(page);
  await searchByAddress(page, 0);
  await setTimeRange(page, "15", "60");
}

test("(9) 探索をキャンセルすると文言が出て次の探索を開始できる", async ({ page }) => {
  // 最初の graph.json 取得だけを 11 秒遅延させ、探索中の状態を保持する。
  // 2 回目以降は即座に fulfill して再検索が成立するようにする。
  const abort = new AbortController();
  productDelayAbort = abort;
  let firstGraphRequest = true;
  await page.route(PRODUCT_GRAPH_URL, async (route) => {
    const res = await route.fetch();
    const body = await res.text();
    if (firstGraphRequest) {
      firstGraphRequest = false;
      await delayUnlessAbortedProduct(11_000, abort.signal);
    }
    if (abort.signal.aborted && firstGraphRequest) {
      return;
    }
    await route.fulfill({ response: res, body });
  });

  await prepareSearch(page);
  await page.click("#search-btn");

  // 探索中はキャンセルボタンが可視になる。
  const cancel = page.locator("#cancel-btn");
  await expect(cancel).toBeVisible();
  await cancel.click();

  await expect(page.locator("#status")).toContainText("キャンセル");
  // キャンセルで探索ボタンが再び有効になり、キャンセルボタンは隠れる。
  await expect(cancel).toBeHidden();
  await expect(page.locator("#search-btn")).toBeEnabled();

  // 遅延中の初回リクエストを解放し、新たな探索を開始できることを確認する。
  abort.abort();
  await page.click("#search-btn");
  await expect(page.locator("#results .card").first()).toBeVisible();
  await expect(cancel).toBeHidden();
});

test("(10) 地図タイル失敗で出発が無効化され、再読み込みで回復する", async ({ page }) => {
  // GSI タイルを全て abort し、タイル失敗状態を作る。
  await page.route("**/cyberjapandata.gsi.go.jp/**", (route) => route.abort());

  await prepareSearch(page);
  await page.click("#search-btn");
  await expect(page.locator("#results .card").first()).toBeVisible();

  // タイル失敗によりフォールバックが表示され、出発ボタンが無効化される。
  await expect(page.locator("#map-fallback")).toBeVisible();
  await expect(page.locator("#results .card").first().locator(".depart")).toBeDisabled();

  // タイルを復旧させてから再読み込みを押すと、実際のタイル読込で回復する。
  // unroute の第 2 引数は handler のため、全 route を確実に破棄する unrouteAll を使う。
  await page.unrouteAll({ behavior: "ignoreErrors" });
  await page.click("#map-retry-btn");
  await expect(page.locator("#map-fallback")).toBeHidden();
  await expect(page.locator("#results .card").first().locator(".depart")).toBeEnabled();
});

test("(11) 成果物改ざんで ARTIFACT_MISMATCH と再読み込み導線が出る", async ({ page }) => {
  await page.route(PRODUCT_GRAPH_URL, async (route) => {
    const res = await route.fetch();
    const body = await res.text();
    await route.fulfill({ response: res, body: `${body}\n` }); // 末尾 1 バイト追加で不一致
  });
  await openApp(page);
  await setTimeRange(page, "15", "60");
  await page.click("#search-btn");

  await expect(page.locator("#status")).toContainText("ARTIFACT_MISMATCH");
  await expect(page.locator("#recovery-actions")).toBeVisible();
  await expect(
    page.locator("#recovery-actions button", { hasText: "再読み込み" }),
  ).toBeVisible();
});

test("(12) キーボードで候補選択でき、出発ボタンの Enter は奪われない", async ({ page }) => {
  await prepareSearch(page);
  await page.click("#search-btn");

  const cards = page.locator("#results .card");
  await expect(cards).toHaveCount(2);
  await expect(cards.nth(0)).toHaveAttribute("aria-current", "true");

  // 2 枚目のカードにフォーカスして Enter → 選択が移る。
  await cards.nth(1).focus();
  await page.keyboard.press("Enter");
  await expect(cards.nth(1)).toHaveAttribute("aria-current", "true");
  await expect(cards.nth(0)).toHaveAttribute("aria-current", "false");

  // 出発ボタンは子要素のため、Enter がカード選択に食われず Maps へ遷移する。
  await page.evaluate(() => {
    const w = window as unknown as {
      __openCalls: string[][];
      open: (...args: string[]) => unknown;
    };
    w.__openCalls = [];
    w.open = (...args: string[]) => {
      w.__openCalls.push(args);
      return null;
    };
  });
  const depart = cards.nth(1).locator(".depart");
  await depart.focus();
  await page.keyboard.press("Enter");

  const calls = await page.evaluate(
    () => (window as unknown as { __openCalls: string[][] }).__openCalls,
  );
  expect(calls).toHaveLength(1);
  expect(calls[0]?.[0]?.startsWith("https://www.google.com/maps/dir/?api=1")).toBe(true);
});

test("(13) 住所検索が 0 件なら見つからない文言を出し時間入力を保持する", async ({ page }) => {
  await stubGeocode(page, { candidates: [] });
  await openApp(page);
  await setTimeRange(page, "30", "90");

  await page.fill("#address-query", "存在しない住所");
  await page.click("#address-search-btn");

  await expect(page.locator("#address-feedback")).toContainText("見つかりませんでした");
  await expect(page.locator("#min-minutes")).toHaveValue("30");
  await expect(page.locator("#max-minutes")).toHaveValue("90");
});

test("(14) 候補カードは最大 3 件（slice(0,3) で上限）", async ({ page }) => {
  await prepareSearch(page);
  await page.click("#search-btn");
  await expect(page.locator("#results .card").first()).toBeVisible();

  const count = await page.locator("#results .card").count();
  expect(count).toBeGreaterThanOrEqual(1);
  expect(count).toBeLessThanOrEqual(3); // App side caps: result.candidates.slice(0, 3)
});

test("(15) 条件変更で結果が失効し再検索を促す", async ({ page }) => {
  await prepareSearch(page);
  await page.click("#search-btn");
  await expect(page.locator("#results .card").first()).toBeVisible();

  // change リスナーは blur で発火するため、入力後に明示的にフォーカスを外す。
  await page.locator("#min-minutes").fill("20");
  await page.locator("#max-minutes").focus();
  await expect(page.locator("#results .card")).toHaveCount(0);
  await expect(page.locator("#status")).toContainText("条件が変更");
});

test("(16) 対応範囲外の出発地点は対応範囲を示し、有効な地点へ復帰できる", async ({ page }) => {
  // C1 最寄り入口から直線 30km 超の地点を使う（新仕様の NO_CONNECTION 条件）。
  // 上野（35.70, 139.77）は C1 から約 2km しか離れておらず候補が返るようになったため、
  // 茨城県つくば市周辺（36.1, 140.1）に変更。C1 最寄り入口まで約 50km 以上ある。
  const outside = [{ label: "茨城県つくば市天王台", lat: 36.1, lon: 140.1 }];
  await stubGeocode(page, { candidates: outside });
  await openApp(page);
  await page.fill("#address-query", "東京都台東区上野5-3-6");
  await page.click("#address-search-btn");
  await page.locator("#address-candidates input[type=radio]").first().check();
  await setTimeRange(page, "15", "60");
  await page.click("#search-btn");

  // NO_CONNECTION の説明文に対応範囲が含まれ、復帰導線が出る。
  await expect(page.locator("#status")).toContainText("対応範囲外");
  await expect(page.locator("#status")).toContainText("都心環状線");
  const recovery = page.locator("#recovery-actions");
  await expect(recovery).toBeVisible();
  const preset = recovery.locator("button", { hasText: "神田橋を出発地点にする" });
  await expect(preset).toBeVisible();

  // ワンタップで有効な出発地点（神田橋）に切り替えると再検索で候補が出る。
  await preset.click();
  await expect(page.locator("#origin-summary")).toContainText("35.68967");
  await page.click("#search-btn");
  await expect(page.locator("#results .card").first()).toBeVisible();
});

// --- 広域（23 区 + 多摩）座標の境界と地図追従（web-001 追加分） ---

/** 座標入力で出発地点を確定する（キーボード利用者と等価な手段）。 */
async function setCoordinateOrigin(page: Page, lat: string, lon: string): Promise<void> {
  await page.fill("#lat", lat);
  await page.fill("#lon", lon);
  // change は blur で発火するため、検索ボタンへフォーカスを移して確定させる。
  await page.locator("#search-btn").focus();
  await expect(page.locator("#origin-summary")).toContainText("座標入力");
}

/** 座標を直接指定して探索する。 */
async function searchFromCoordinate(
  page: Page,
  lat: string,
  lon: string,
  min: string,
  max: string,
): Promise<void> {
  await openApp(page);
  await setCoordinateOrigin(page, lat, lon);
  await setTimeRange(page, min, max);
  await page.click("#search-btn");
}

test("(17) 立川駅（29.6km）は 15〜240 分で候補が返る", async ({ page }) => {
  // 従来の 30km 固定 cap では NO_CONNECTION だった地点。46km cap + 時間窓で成立する。
  await searchFromCoordinate(page, "35.6979", "139.4139", "15", "240");

  const firstCard = page.locator("#results .card").first();
  await expect(firstCard).toBeVisible();
  // アクセスが長い地点なので「入り」の内訳が表示され、直線距離も出る。
  await expect(firstCard).toContainText("入り");
  await expect(firstCard).toContainText("入口まで（直線）");
});

test("(18) 八王子駅（36.1km）は 15〜240 分でも候補なしで数値理由を出す", async ({ page }) => {
  await searchFromCoordinate(page, "35.6556", "139.3388", "15", "240");

  await expect(page.locator("#results .card")).toHaveCount(0);
  const status = page.locator("#status");
  // 最短計画が 240 分を超えるため、時間枠ではなく距離・時間の数値で説明する。
  await expect(status).toContainText("最大 4 時間では周回できません");
  // 240 分超の値は列挙範囲（ループ部分 ≤ 240 分）での最小なので「最短」とは断定しない。
  await expect(status).toContainText("確認できた範囲で最も短い計画時間は");
  await expect(status).not.toContainText("最短でも");
  await expect(status).toContainText("km");

  // 時間を広げても届かないので「時間の上限を広げる」は出さない。
  await expect(page.locator("#recovery-actions")).toBeVisible();
  await expect(page.locator("#recovery-actions button", { hasText: "時間の上限を広げる" })).toHaveCount(0);
  // 有効な地点へ切り替える導線は残す。
  await expect(
    page.locator("#recovery-actions button", { hasText: "神田橋を出発地点にする" }),
  ).toBeVisible();
});

test("(19) 奥多摩（cap 超）は対応範囲外と最寄り入口の距離を示す", async ({ page }) => {
  // C1 最寄り入口が 46km cap を超える地点。NO_CONNECTION + nearestAccess を表示する。
  await searchFromCoordinate(page, "35.8106", "139.0937", "15", "240");

  await expect(page.locator("#results .card")).toHaveCount(0);
  const status = page.locator("#status");
  await expect(status).toContainText("対応範囲外");
  await expect(status).toContainText("最寄り入口まで直線");
  await expect(status).toContainText("km");
  await expect(status).toContainText("都心環状線");
  await expect(page.locator("#recovery-actions")).toBeVisible();
});

test("(20) 立川駅でも指定枠 60 分なら従来どおり時間枠を広げる導線のまま", async ({ page }) => {
  await searchFromCoordinate(page, "35.6979", "139.4139", "15", "60");

  await expect(page.locator("#results .card")).toHaveCount(0);
  await expect(page.locator("#status")).toContainText("時間枠を広げる");
  const widen = page.locator("#recovery-actions button", { hasText: "時間の上限を広げる" });
  await expect(widen).toBeVisible();
  await widen.click();
  await expect(page.locator("#max-minutes")).toHaveValue("90");
});

test("(21) 座標確定で地図が追従し、マーカーが表示範囲内に入る", async ({ page }) => {
  await searchFromCoordinate(page, "35.6979", "139.4139", "15", "240");
  await expect(page.locator("#results .card").first()).toBeVisible();

  // pan/zoom はアニメーションするため、収束するまでポーリングして確認する。
  await expect(async () => {
    const mapBox = await page.locator("#map").boundingBox();
    const markerBox = await page.locator("#map .origin-marker").boundingBox();
    expect(mapBox).not.toBeNull();
    expect(markerBox).not.toBeNull();
    if (mapBox === null || markerBox === null) {
      return;
    }
    expect(markerBox.x).toBeGreaterThanOrEqual(mapBox.x - 1);
    expect(markerBox.y).toBeGreaterThanOrEqual(mapBox.y - 1);
    expect(markerBox.x + markerBox.width).toBeLessThanOrEqual(mapBox.x + mapBox.width + 1);
    expect(markerBox.y + markerBox.height).toBeLessThanOrEqual(mapBox.y + mapBox.height + 1);
  }).toPass({ timeout: 5000 });
});

test("(22) 地図タップで指定した地点を確定して探索できる", async ({ page }) => {
  await stubGeocode(page, { candidates: CANDIDATES });
  await openApp(page);

  // 専用モードに入ってからタップする（ドラッグ・ズームと競合しない）。
  await page.click("#map-pick-btn");
  await expect(page.locator("#map-pick-panel")).toBeVisible();
  await expect(page.locator("#map-pick-confirm")).toBeDisabled();

  const mapBox = await page.locator("#map").boundingBox();
  expect(mapBox).not.toBeNull();
  if (mapBox !== null) {
    // 都心（初期中心）付近をタップする。
    await page.locator("#map").click({
      position: { x: Math.round(mapBox.width / 2), y: Math.round(mapBox.height / 2) },
    });
  }
  await expect(page.locator("#map-pick-status")).toContainText("候補地点");
  await expect(page.locator("#map .pending-marker")).toBeAttached();
  await expect(page.locator("#map-pick-confirm")).toBeEnabled();

  await page.click("#map-pick-confirm");
  // 逆ジオコーディングはしないので座標ラベルで示す。
  await expect(page.locator("#origin-summary")).toContainText("地図で指定（住所は未取得）");
  await expect(page.locator("#origin-summary")).toContainText("35.");
  await expect(page.locator("#map-pick-panel")).toBeHidden();

  await setTimeRange(page, "15", "60");
  await page.click("#search-btn");
  await expect(page.locator("#results .card").first()).toBeVisible();
});

test("(23) 地図タップ指定は Esc でキャンセルできる", async ({ page }) => {
  await openApp(page);
  await page.click("#map-pick-btn");
  const mapBox = await page.locator("#map").boundingBox();
  if (mapBox !== null) {
    await page.locator("#map").click({
      position: { x: Math.round(mapBox.width / 2), y: Math.round(mapBox.height / 2) },
    });
  }
  await expect(page.locator("#map-pick-confirm")).toBeEnabled();
  await page.keyboard.press("Escape");
  await expect(page.locator("#map-pick-panel")).toBeHidden();
  await expect(page.locator("#map .pending-marker")).toHaveCount(0);
  // 出発地点は神田橋（起動時プリセット）のまま変わらない。
  await expect(page.locator("#origin-summary")).toContainText("神田橋");
});

// --- correct-001: GPS 競合・タップ UI フォーカス・復帰導線（レビュー指摘の回帰） ---

/** getCurrentPosition を保留し、テストから任意のタイミングで成功させるスタブ。 */
async function stubGeolocationDeferred(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const w = window as unknown as {
      __geoCallCount: number;
      __geoResolve: ((p: unknown) => void) | null;
    };
    w.__geoCallCount = 0;
    w.__geoResolve = null;
    Object.defineProperty(navigator, "geolocation", {
      configurable: true,
      value: {
        getCurrentPosition: (onSuccess: (p: unknown) => void, _onError?: (e: unknown) => void) => {
          w.__geoCallCount += 1;
          w.__geoResolve = (p: unknown) => {
            onSuccess(p);
          };
        },
      },
    });
  });
}

test("(24) 現在地取得中に地図で確定すると、遅れた現在地は出発地点を上書きしない", async ({ page }) => {
  // 地図追従のアニメーションを止め、確定地点のマーカー位置を決定的に比較する。
  await page.emulateMedia({ reducedMotion: "reduce" });
  await stubGeolocationDeferred(page);
  await openApp(page);

  // 現在地取得を開始して保留させる。
  await page.click("#geolocate-btn");
  await expect(page.locator("#geolocate-btn")).toHaveText("取得中…");

  // 取得待ちの間に地図タップで出発地点を確定する（supersede）。
  await page.click("#map-pick-btn");
  const mapBox = await page.locator("#map").boundingBox();
  expect(mapBox).not.toBeNull();
  if (mapBox !== null) {
    await page.locator("#map").click({
      position: { x: Math.round(mapBox.width / 2), y: Math.round(mapBox.height / 2) },
    });
  }
  await expect(page.locator("#map-pick-confirm")).toBeEnabled();
  await page.click("#map-pick-confirm");
  await expect(page.locator("#origin-summary")).toContainText("地図で指定（住所は未取得）");

  // 確定時点の状態を記録する（遅延現在地 35.0, 135.0 とは異なること）。
  const pickedLat = await page.locator("#lat").inputValue();
  const pickedLon = await page.locator("#lon").inputValue();
  expect(pickedLat).not.toBe("35");
  expect(pickedLon).not.toBe("135");
  const originMarker = page.locator("#map .origin-marker");
  await expect(originMarker).toHaveCount(1);
  const markerBefore = await originMarker.boundingBox();

  // 確定後に現在地取得成功が届いても、確定済みの出発地点を上書きしない（SEC-01）。
  await page.evaluate(
    (p) => {
      (window as unknown as { __geoResolve?: (x: unknown) => void }).__geoResolve?.(p);
    },
    { coords: { latitude: 35.0, longitude: 135.0, accuracy: 5 }, timestamp: Date.now() },
  );
  // 遅延コールバックが完了するまで待つ（取得中… の解除）。成功前から成立する
  // アサーションだけで pass しないよう、完了を明示的に待ってから状態を検証する。
  await expect(page.locator("#geolocate-btn")).toBeEnabled();
  await expect(page.locator("#geolocate-btn")).toHaveText("現在地を使う");

  await expect(page.locator("#origin-summary")).toContainText("地図で指定（住所は未取得）");
  await expect(page.locator("#origin-summary")).not.toContainText("（現在地）");
  // 座標入力も確定地点のまま（現在地の緯度経度へ化けていない）。
  await expect(page.locator("#lat")).toHaveValue(pickedLat);
  await expect(page.locator("#lon")).toHaveValue(pickedLon);
  // 出発地マーカーも確定地点に留まる（現在地へ移動していない）。
  const markerAfter = await originMarker.boundingBox();
  expect(markerAfter).not.toBeNull();
  if (markerBefore !== null && markerAfter !== null) {
    expect(Math.abs(markerAfter.x - markerBefore.x)).toBeLessThanOrEqual(2);
    expect(Math.abs(markerAfter.y - markerBefore.y)).toBeLessThanOrEqual(2);
  }
});

test("(25) 地図タップ指定の確定/取消が視界に入り、フォーカスが論理的な起点へ戻る", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 700 });
  await openApp(page);

  await page.click("#map-pick-btn");
  // F2: disabled の確定ボタンではなく、地図直下の状態文へフォーカスする。
  expect(await page.evaluate(() => document.activeElement?.id)).toBe("map-pick-status");
  await expect(page.locator("#map-pick-panel")).toBeVisible();
  // F6: モード中の地図には枠の手掛かりが付く。
  await expect(page.locator("#map")).toHaveClass(/map--pick/);

  const mapBox = await page.locator("#map").boundingBox();
  expect(mapBox).not.toBeNull();
  if (mapBox !== null) {
    await page.locator("#map").click({
      position: { x: Math.round(mapBox.width / 2), y: Math.round(mapBox.height / 2) },
    });
  }
  await expect(page.locator("#map-pick-confirm")).toBeEnabled();
  // F1: タップ後に確定ボタンがビューポート内に入る。
  await expect(page.locator("#map-pick-confirm")).toBeInViewport();

  await page.click("#map-pick-confirm");
  // F3: 確定後は出発地点の要約（新しい起点）へフォーカスが戻る。
  expect(await page.evaluate(() => document.activeElement?.id)).toBe("origin-summary");

  // 取消でもフォーカスが起動ボタンへ戻る。
  await page.click("#map-pick-btn");
  expect(await page.evaluate(() => document.activeElement?.id)).toBe("map-pick-status");
  await page.click("#map-pick-cancel");
  expect(await page.evaluate(() => document.activeElement?.id)).toBe("map-pick-btn");
});

test("(26) 候補ゼロの復帰導線がモバイル幅で視界に入りフォーカスされる", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 700 });
  // 八王子駅: 240 分でも届かない到達不能ケース。
  await searchFromCoordinate(page, "35.6556", "139.3388", "15", "240");

  await expect(page.locator("#results .card")).toHaveCount(0);
  const recovery = page.locator("#recovery-actions");
  await expect(recovery).toBeVisible();
  // F5: 地図の下にあっても可視位置へスクロールされる。
  await expect(recovery).toBeInViewport();
  // F5: 主操作へフォーカスが移る。
  const focusInRecovery = await page.evaluate(() => {
    const region = document.getElementById("recovery-actions");
    return region !== null && region.contains(document.activeElement);
  });
  expect(focusInRecovery).toBe(true);
  // 到達不能なので「時間の上限を広げる」は出さない（誤った復帰導線の防止）。
  await expect(recovery.locator("button", { hasText: "時間の上限を広げる" })).toHaveCount(0);
});

test("(27) prefers-reduced-motion では座標確定の地図追従がアニメーションせず即座に収まる", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openApp(page);
  await setCoordinateOrigin(page, "35.6979", "139.4139");

  // animate:false なのでアニメーション収束を待たずにマーカーが表示範囲内に入る。
  const mapBox = await page.locator("#map").boundingBox();
  const markerBox = await page.locator("#map .origin-marker").boundingBox();
  expect(mapBox).not.toBeNull();
  expect(markerBox).not.toBeNull();
  if (mapBox !== null && markerBox !== null) {
    expect(markerBox.x).toBeGreaterThanOrEqual(mapBox.x - 1);
    expect(markerBox.y).toBeGreaterThanOrEqual(mapBox.y - 1);
    expect(markerBox.x + markerBox.width).toBeLessThanOrEqual(mapBox.x + mapBox.width + 1);
    expect(markerBox.y + markerBox.height).toBeLessThanOrEqual(mapBox.y + mapBox.height + 1);
  }
});
