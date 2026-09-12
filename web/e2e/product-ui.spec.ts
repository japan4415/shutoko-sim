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
