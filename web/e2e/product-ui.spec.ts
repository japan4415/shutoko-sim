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
  await expect(cards).toHaveCount(1);
  // 先頭候補が初期選択される。
  await expect(cards.nth(0)).toHaveAttribute("aria-current", "true");

  await cards.nth(0).click();
  await expect(cards.nth(0)).toHaveAttribute("aria-current", "true");

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
  await expect(cards).toHaveCount(1);
  const badges = page.locator("#results .card .candidate-index");
  await expect(badges).toHaveCount(1);
  await expect(badges.nth(0)).toHaveText("1");
});

// --- issue #15 追加の must-have テスト（レビュー指摘の未カバー経路） ---

const PRODUCT_GRAPH_URL = "**/releases/*/graph.json";

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
  await expect(cards).toHaveCount(1);
  await expect(cards.nth(0)).toHaveAttribute("aria-current", "true");

  // カードにフォーカスして Enter → 選択維持。
  await cards.nth(0).focus();
  await page.keyboard.press("Enter");
  await expect(cards.nth(0)).toHaveAttribute("aria-current", "true");

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
  const depart = cards.first().locator(".depart");
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
  // 最寄り入口から直線 46km 超の地点を使う（NO_CONNECTION 条件）。
  const outside = [{ label: "茨城県水戸市三の丸", lat: 36.37, lon: 140.47 }];
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

test("(28) 240/240 では上限を広げず、最小時間を下げる導線で値が実際に変わり再検索できる", async ({ page }) => {
  // 大手町: 確認できた範囲で最も短い周回は約 28 分（plan 1696s）で下限 240 分に届かない。
  // 上限は既に製品上限 240 分なので「時間の上限を広げる」を出さず、最小時間を下げて
  // 実際に値を変更する（review R2-01: 240 分へ「広げました」と偽る旧導線の回帰防止）。
  await searchFromCoordinate(page, "35.6866", "139.7643", "240", "240");

  await expect(page.locator("#results .card")).toHaveCount(0);
  await expect(page.locator("#status")).toContainText("候補がありません");
  const recovery = page.locator("#recovery-actions");
  await expect(recovery).toBeVisible();
  // 上限 240 分では拡大操作（値が変わらない）を出さない。
  await expect(recovery.locator("button", { hasText: "時間の上限を広げる" })).toHaveCount(0);
  const lower = recovery.locator("button", { hasText: "最小時間を 23 分に下げる" });
  await expect(lower).toBeVisible();

  await lower.click();
  // 値が実際に変わる（240 → 23）。上限は変わらない。
  await expect(page.locator("#min-minutes")).toHaveValue("23");
  await expect(page.locator("#max-minutes")).toHaveValue("240");

  // 次の検索が実行でき、下限を下げたことで候補が返る。
  await page.click("#search-btn");
  await expect(page.locator("#results .card").first()).toBeVisible();
});

test("(29) 240/240 の復帰導線は最小時間を手入力すると失効し、そのまま再検索できる", async ({ page }) => {
  // 復帰ボタンと同じ 23 を手入力（change は blur で発火）。古い「下げる」ボタンが
  // 残ると 23→23 の no-op を『下げました』と偽る（review R3-01）。
  await searchFromCoordinate(page, "35.6866", "139.7643", "240", "240");
  const recovery = page.locator("#recovery-actions");
  await expect(recovery).toBeVisible();
  await expect(
    recovery.locator("button", { hasText: "最小時間を 23 分に下げる" }),
  ).toBeVisible();

  await page.locator("#min-minutes").fill("23");
  await page.locator("#max-minutes").focus(); // change を確定させる
  await expect(recovery).toBeHidden();
  await expect(page.locator("#status")).toContainText("条件が変更");

  // 手入力した条件のまま再検索でき、候補が返る。
  await page.click("#search-btn");
  await expect(page.locator("#results .card").first()).toBeVisible();
});

test("(30) 復帰ボタンより小さい 15 を手入力しても古い導線は残らず、引上げを成功と告げない", async ({ page }) => {
  // 23 より小さい 15 を手入力すると、残ったボタンは 15→23 の引上げになる（review R3-01）。
  await searchFromCoordinate(page, "35.6866", "139.7643", "240", "240");
  const recovery = page.locator("#recovery-actions");
  await expect(recovery).toBeVisible();

  await page.locator("#min-minutes").fill("15");
  await page.locator("#max-minutes").focus();
  await expect(recovery).toBeHidden();
  await expect(page.locator("#min-minutes")).toHaveValue("15");
  await expect(page.locator("#status")).not.toContainText("下げました");

  await page.click("#search-btn");
  await expect(page.locator("#results .card").first()).toBeVisible();
});

test("(31) 復帰ボタン押下時も現在値と比較し、引上げや no-op を成功と告げない", async ({ page }) => {
  await searchFromCoordinate(page, "35.6866", "139.7643", "240", "240");
  const recovery = page.locator("#recovery-actions");
  const lower = recovery.locator("button", { hasText: "最小時間を 23 分に下げる" });
  await expect(lower).toBeVisible();

  // change を発火させずに値を 15 へ変える（描画後に現在値が変わった状態を模す）。
  await page.evaluate(() => {
    const input = document.getElementById("min-minutes");
    if (input instanceof HTMLInputElement) {
      input.value = "15";
    }
  });
  await lower.click();

  // 15 → 23 の引上げは行わず、成功も告げない。値は手入力のまま。
  await expect(page.locator("#min-minutes")).toHaveValue("15");
  await expect(page.locator("#status")).toContainText("ままです");
  await expect(page.locator("#status")).not.toContainText("下げました");
});

test("(32) 成果物不一致の再読み込み案内は条件変更では消えない", async ({ page }) => {
  await page.route(PRODUCT_GRAPH_URL, async (route) => {
    const res = await route.fetch();
    const body = await res.text();
    await route.fulfill({ response: res, body: `${body}\n` }); // 末尾 1 バイト追加で不一致
  });
  await openApp(page);
  await setTimeRange(page, "15", "60");
  await page.click("#search-btn");

  const recovery = page.locator("#recovery-actions");
  await expect(recovery).toBeVisible();
  await expect(recovery).toContainText("成果物を読み込めませんでした");

  // 結果取得前の再読み込み案内は前回結果に基づかないため、条件変更でも残す。
  await page.locator("#min-minutes").fill("20");
  await page.locator("#max-minutes").focus();
  await expect(recovery).toBeVisible();
  await expect(
    recovery.locator("button", { hasText: "再読み込み" }),
  ).toBeVisible();
});

// --- 全首都高ランプ選択 UI（R2）の E2E 検証 ---

test("(33) 明示指定モード切替、全399件確認、197件選択可能（入口99/出口98）、探索ボタン制御", async ({ page }) => {
  await openApp(page);

  // デフォルトは「現在地・住所から」モード、明示指定セクションは非表示
  const explicitSection = page.locator("#explicit-od-section");
  await expect(explicitSection).toBeHidden();
  await expect(page.locator('input[name="search-mode"][value="coord"]')).toBeChecked();

  // 明示指定モードに切り替え
  await page.click('input[name="search-mode"][value="explicit"]');
  await expect(explicitSection).toBeVisible();

  // 台帳の読み込みステータス確認
  const loadingStatus = page.locator("#ramps-loading-status");
  await expect(loadingStatus).toBeVisible();
  await expect(loadingStatus).toContainText("正規ランプ台帳 399 件を検証完了");

  // 入口・出口それぞれの件数と選択可能件数の整合性
  // 入口: 全399件表示、99件が選択可能
  const entryItems = page.locator("#entry-ramp-list .ramp-item");
  await expect(entryItems).toHaveCount(399);
  const entrySelectable = page.locator('#entry-ramp-list input[type="radio"]:not([disabled])');
  await expect(entrySelectable).toHaveCount(99);
  const entryDisabled = page.locator('#entry-ramp-list input[type="radio"][disabled]');
  await expect(entryDisabled).toHaveCount(300);
  await expect(page.locator("#entry-count-info")).toContainText("全 399 件（選択可能 99 件）");

  // 出口: 全399件表示、98件が選択可能
  const exitItems = page.locator("#exit-ramp-list .ramp-item");
  await expect(exitItems).toHaveCount(399);
  const exitSelectable = page.locator('#exit-ramp-list input[type="radio"]:not([disabled])');
  await expect(exitSelectable).toHaveCount(98);
  const exitDisabled = page.locator('#exit-ramp-list input[type="radio"][disabled]');
  await expect(exitDisabled).toHaveCount(301);
  await expect(page.locator("#exit-count-info")).toContainText("全 399 件（選択可能 98 件）");

  // 合計選択可能件数は 197 (99 + 98)
  expect((await entrySelectable.count()) + (await exitSelectable.count())).toBe(197);

  // 入口・出口が未選択なので探索ボタンは無効化されている
  const searchBtn = page.locator("#search-btn");
  await expect(searchBtn).toBeDisabled();
  await expect(page.locator("#explicit-selection-status")).toContainText(
    "入口と出口が未選択です",
  );
});

test("(34) ランプ検索（施設名/路線/方向/ID）、絞り込み件数表示、0件メッセージと復帰ボタン", async ({ page }) => {
  await openApp(page);
  await page.click('input[name="search-mode"][value="explicit"]');
  await expect(page.locator("#ramps-loading-status")).toContainText("399 件を検証完了");

  // 入口検索: 施設名「銀座」で絞り込み
  const entrySearch = page.locator("#entry-ramp-search");
  await entrySearch.fill("銀座");
  await expect(page.locator("#entry-count-info")).toContainText("該当 4 件");
  await expect(page.locator("#entry-clear-search-btn")).toBeVisible();

  // クリアボタンで絞り込み解除
  await page.click("#entry-clear-search-btn");
  await expect(entrySearch).toHaveValue("");
  await expect(page.locator("#entry-count-info")).toContainText("全 399 件（選択可能 99 件）");

  // 路線記号「C1」で絞り込み
  await entrySearch.fill("C1");
  await expect(page.locator("#entry-ramp-list .ramp-item").first()).toBeVisible();

  // 方向「内回」で絞り込み
  await entrySearch.fill("内回");
  await expect(page.locator("#entry-ramp-list .ramp-item").first()).toBeVisible();

  // ランプID「ramp:c1-inner:ginza-entry」で絞り込み
  await entrySearch.fill("ramp:c1-inner:ginza-entry");
  await expect(page.locator("#entry-ramp-list .ramp-item")).toHaveCount(1);
  await expect(page.locator("#entry-ramp-list .ramp-item")).toContainText("銀座");

  // 存在しないランプ名で検索 → 0件メッセージとリセットボタン
  await entrySearch.fill("存在しない架空ランプ999");
  await expect(page.locator("#entry-zero-message")).toBeVisible();
  await expect(page.locator("#entry-ramp-list")).toBeHidden();
  await expect(page.locator("#entry-count-info")).toContainText("0 件");

  // リセットボタンで復帰
  await page.click("#entry-reset-filter-btn");
  await expect(page.locator("#entry-zero-message")).toBeHidden();
  await expect(page.locator("#entry-ramp-list")).toBeVisible();
  await expect(entrySearch).toHaveValue("");
  await expect(page.locator("#entry-count-info")).toContainText("全 399 件（選択可能 99 件）");
});

test("(35) 非対応・無効ランプの理由（周回不可・未対応・境界JCT・閉鎖・役割不一致）が明示される", async ({ page }) => {
  await openApp(page);
  await page.click('input[name="search-mode"][value="explicit"]');
  await expect(page.locator("#ramps-loading-status")).toContainText("399 件を検証完了");

  // 入口側での各無効分類の確認
  const entrySearch = page.locator("#entry-ramp-search");

  // 1. 周回不可 (structural_no_loop): 池尻（3号渋谷線下り、5km循環SCCへ到達不可）
  await entrySearch.fill("ramp:3-outbound:ikejiri-entry");
  const ikejiri = page.locator("#entry-ramp-list .ramp-item").first();
  await expect(ikejiri).toBeVisible();
  await expect(ikejiri.locator(".ramp-status-badge")).toContainText("周回不可");
  await expect(ikejiri.locator(".ramp-disabled-reason")).toContainText("循環SCC");
  await expect(ikejiri.locator('input[type="radio"]')).toBeDisabled();

  // 2. 未対応 (unsupported): 139件のいずれか（例: 芝公園入口内回り）
  await entrySearch.fill("ramp:c1-inner:shibakoen-entry");
  const shibakoenIn = page.locator("#entry-ramp-list .ramp-item").first();
  await expect(shibakoenIn).toBeVisible();
  await expect(shibakoenIn.locator(".ramp-status-badge")).toContainText("未対応");
  await expect(shibakoenIn.locator(".ramp-disabled-reason")).toContainText("未対応");
  await expect(shibakoenIn.locator('input[type="radio"]')).toBeDisabled();

  // 3. 役割不一致 (出口専用): 入口リストで出口ランプを探す
  await entrySearch.fill("ramp:c1-outer:shibakoen-exit");
  const shibakoenOut = page.locator("#entry-ramp-list .ramp-item").first();
  await expect(shibakoenOut).toBeVisible();
  await expect(shibakoenOut.locator(".ramp-status-badge")).toContainText("出口専用");
  await expect(shibakoenOut.locator(".ramp-disabled-reason")).toContainText("出口専用ランプ");
  await expect(shibakoenOut.locator('input[type="radio"]')).toBeDisabled();

  // 4. 境界JCT (boundary):
  await entrySearch.fill("外環接続美女木");
  const gaikan = page.locator("#entry-ramp-list .ramp-item").first();
  await expect(gaikan).toBeVisible();
  await expect(gaikan.locator(".ramp-status-badge")).toContainText("境界JCT");
  await expect(gaikan.locator(".ramp-disabled-reason")).toContainText("境界");
  await expect(gaikan.locator('input[type="radio"]')).toBeDisabled();
});

test("(36) 明示OD選択、探索ボタン有効化、探索実行、重複探索警告、解除操作", async ({ page }) => {
  await openApp(page);
  await page.click('input[name="search-mode"][value="explicit"]');
  await expect(page.locator("#ramps-loading-status")).toContainText("399 件を検証完了");

  const searchBtn = page.locator("#search-btn");
  await expect(searchBtn).toBeDisabled();

  // 入口ランプ「ramp:c1-inner:ginza-entry」を選択
  await page.locator("#entry-ramp-search").fill("ramp:c1-inner:ginza-entry");
  const entryRadio = page.locator('#entry-ramp-list input[type="radio"]:not([disabled])').first();
  await entryRadio.check();

  // 入口バッジが表示されるが、出口未選択のためボタンは無効のまま
  await expect(page.locator("#entry-selected-badge")).toBeVisible();
  await expect(page.locator("#entry-selected-name")).toContainText("銀座");
  await expect(searchBtn).toBeDisabled();
  await expect(page.locator("#explicit-selection-status")).toContainText("出口が未選択です");

  // 出口ランプ「ramp:c1-outer:shibakoen-exit」を選択
  await page.locator("#exit-ramp-search").fill("ramp:c1-outer:shibakoen-exit");
  const exitRadio = page.locator('#exit-ramp-list input[type="radio"]:not([disabled])').first();
  await exitRadio.check();

  // 出口バッジが表示され、両方選択されたのでボタンが有効化
  await expect(page.locator("#exit-selected-badge")).toBeVisible();
  await expect(page.locator("#exit-selected-name")).toContainText("芝公園");
  await expect(searchBtn).toBeEnabled();
  await expect(page.locator("#explicit-selection-status")).toContainText(
    "「ルートを探す」を押して探索を開始できます",
  );

  // 時間範囲を設定して探索実行
  await setTimeRange(page, "15", "60");
  await searchBtn.click();

  // 結果カードが表示される
  await expect(page.locator("#results .card").first()).toBeVisible({ timeout: 15_000 });
  await expect(page.locator("#results .card").first()).toContainText("銀座");

  // 同一条件で再度「ルートを探す」を押下 → 重複警告が表示される
  await searchBtn.click();
  const dupWarning = page.locator("#duplicate-warning");
  await expect(dupWarning).toBeVisible();
  await expect(dupWarning).toContainText("同じ結果が既に表示されています");

  // 入口解除ボタンをクリック → バッジが消え、探索ボタンが無効化され、重複警告も消える
  await page.click("#entry-deselect-btn");
  await expect(page.locator("#entry-selected-badge")).toBeHidden();
  await expect(searchBtn).toBeDisabled();
  await expect(dupWarning).toBeHidden();
});

test("(37) モバイル表示（375px）で横スクロール（overflow-x）が発生しない", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 667 });
  await openApp(page);
  await page.click('input[name="search-mode"][value="explicit"]');
  await expect(page.locator("#ramps-loading-status")).toContainText("399 件を検証完了");

  // 水平スクロールが発生していないことを確認
  const hasHorizontalScroll = await page.evaluate(() => {
    return document.documentElement.scrollWidth > document.documentElement.clientWidth;
  });
  expect(hasHorizontalScroll).toBe(false);

  // ランプアイテムがタップしやすい十分な高さを確保していること（最低44px）
  const firstItem = page.locator("#entry-ramp-list .ramp-item").first();
  const box = await firstItem.boundingBox();
  expect(box).not.toBeNull();
  expect(box!.height).toBeGreaterThanOrEqual(44);
});

test("(38) ランプ台帳の読み込み失敗時にエラーパネルと再試行ボタンが表示される", async ({ page }) => {
  // ramps.json の読み込みを 500 エラーにする
  await page.route("**/releases/*/ramps.json*", async (route) => {
    await route.fulfill({
      status: 500,
      contentType: "text/plain",
      body: "Internal Server Error",
    });
  });

  await openApp(page);
  await page.click('input[name="search-mode"][value="explicit"]');

  const errorPanel = page.locator("#ramps-error-panel");
  await expect(errorPanel).toBeVisible();
  await expect(page.locator("#ramps-error-message")).toContainText("ランプ台帳の読み込みまたは検証に失敗しました");
  const retryBtn = page.locator("#ramps-retry-btn");
  await expect(retryBtn).toBeVisible();

  // route を解除して再試行
  await page.unroute("**/releases/*/ramps.json*");
  await retryBtn.click();

  // 正常に読み込まれ、エラーパネルが消える
  await expect(errorPanel).toBeHidden();
  await expect(page.locator("#ramps-loading-status")).toContainText("399 件を検証完了");
  await expect(page.locator("#entry-ramp-list .ramp-item")).toHaveCount(399);
});
