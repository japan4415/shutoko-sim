// issue #12 完了条件の E2E 検証。
// (a) 神田橋プリセット → 15〜60 → 候補カードと Maps URL / window.open 引数
// (b) 60〜90 分で TIME_WINDOW 診断（Issue #57）
// (c) graph.json 改ざんで ARTIFACT_MISMATCH の文言
// (d) graph.json 11 秒遅延で TIMEOUT の文言
// (e) 入力エラー修正後の再検索
// (f) 240/240 の指定枠で TIME_WINDOW 診断と復帰導線（Issue #57）
import { expect, test } from "@playwright/test";
import { stubTiles } from "./tiles";

const GRAPH_URL = "**/releases/*/graph.json";

/**
 * (d) の 11 秒遅延をテスト終了時に解除するためのコントローラ。
 * 遅延中の route ハンドラがテスト終了後まで残ると、フルスイート実行時に
 * "route.fetch: Test ended." で (d) が不安定に落ちる（verify-001 F2）。
 */
let delayAbort: AbortController | null = null;

/** abort 可能な待機。abort されたら即座に resolve する。 */
function delayUnlessAborted(ms: number, signal: AbortSignal): Promise<void> {
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

async function openApp(page: import("@playwright/test").Page): Promise<void> {
  // 地図タイルはテストの対象ではないので常に差し替える（tiles.ts 参照）。
  await stubTiles(page);
  await page.goto("/");
  await expect(page.locator("#search-btn")).toBeVisible();
  // #15 以降、座標プリセットは折りたたみ内にある。テストからは開いて操作する。
  await page.locator("#manual-input").evaluate((node) => {
    (node as HTMLDetailsElement).open = true;
  });
}

// 各テストの後に route ハンドラを確実に破棄する（pending は ignoreErrors で待たない）。
test.afterEach(async ({ page }) => {
  delayAbort?.abort();
  delayAbort = null;
  await page.unrouteAll({ behavior: "ignoreErrors" });
});

test("(a) 神田橋プリセット 15〜60 で候補カードと Maps URL が表示される", async ({ page }) => {
  await page.clock.setFixedTime(new Date("2026-11-22T01:00:00Z"));
  await openApp(page);
  await page.selectOption("#origin-preset", "kandabashi");
  await page.fill("#min-minutes", "15");
  await page.fill("#max-minutes", "60");
  await page.click("#search-btn");

  const firstCard = page.locator("#results .card").first();
  await expect(firstCard).toBeVisible();
  await expect(page.locator("#results-scope")).toBeVisible();
  await expect(page.locator("#results-scope")).toContainText("すべての入口を比較した結果ではありません");
  await expect(firstCard.locator(".charging")).toHaveText("課金対象: 神田橋入口 → 宝町出口 の1区間");
  await expect(firstCard.locator(".toll")).toContainText("300 円");
  await expect(firstCard.locator(".distance--billing")).toHaveText("料金表上の距離: 1.7 km");
  await expect(firstCard.locator(".distance--shutoko")).toHaveText("首都高距離: 16.1 km");
  await expect(firstCard.locator(".distance--total")).toHaveCount(0);
  await expect(page.locator("#results .card")).toHaveCount(1);
  // 実走行の線は残すが、走行全体を課金線として重ねない。
  await expect(page.locator('#map .leaflet-overlay-pane path[stroke="#0b5cad"]').first()).toBeVisible();
  await expect(page.locator('#map .leaflet-overlay-pane path[stroke="#c2410c"]')).toHaveCount(0);

  await expect(firstCard.locator(".duration")).toHaveText("往復所要時間（余裕なし）: 約24分");
  await expect(firstCard.locator(".plan-time")).toHaveText("計画時間（帰着・余裕込み）: 約29分");
  await expect(firstCard.locator(".breakdown")).toContainText("帰り 4分（概算）");
  await expect(firstCard.locator(".duration-note")).toContainText("出発地点に戻るまで");
  const timeNote = page.locator("#time-note");
  await expect(timeNote).toContainText("同じ地点に戻るまで");
  await expect(timeNote).toContainText("最小時間は余裕なし、最大時間は余裕込み");
  await expect(page.locator("#min-minutes")).toHaveAttribute("aria-describedby", "time-note");
  await expect(page.locator("#max-minutes")).toHaveAttribute("aria-describedby", "time-note");

  const mapsUrl = await firstCard.getAttribute("data-maps-url");
  const url = new URL(mapsUrl ?? "");
  expect(url.origin + url.pathname).toBe("https://www.google.com/maps/dir/");
  expect(url.searchParams.get("api")).toBe("1");
  expect(url.searchParams.get("travelmode")).toBe("driving");
  expect(url.searchParams.get("origin")).toBe("35.689673,139.764425");
  expect(url.searchParams.get("destination")).toBe(url.searchParams.get("origin"));
  // 入口付近 → 周回を誘導する経由地 → 出口の順。外部アプリでの再現性は別に確認する。
  expect(url.searchParams.get("waypoints")?.split("|")).toEqual([
    "35.689673,139.764425", "35.654776,139.744043", "35.677890,139.775197",
  ]);
  await expect(firstCard.locator(".maps-handoff-notice")).toContainText("出発前に");

  // window.open の引数をスタブで検証（外部遷移せず決定論的）。
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
  await firstCard.locator(".depart").click();
  const calls = await page.evaluate(() => {
    const w = window as unknown as { __openCalls: string[][] };
    return w.__openCalls;
  });
  expect(calls).toHaveLength(1);
  expect(calls[0]?.[0]).toBe(mapsUrl);
  expect(calls[0]?.[1]).toBe("_blank");
  expect(calls[0]?.[2]).toBe("noopener");
});

test("(g) 神田橋は帰着と余裕込みで、最大28分には収まらず29分には収まる", async ({ page }) => {
  await page.clock.setFixedTime(new Date("2026-11-22T01:00:00Z"));
  await openApp(page);
  await page.fill("#min-minutes", "23");
  await page.fill("#max-minutes", "28");
  await page.click("#search-btn");
  await expect(page.locator("#status")).toContainText("候補がありません");
  await expect(page.locator("#results-scope")).toBeHidden();
  await expect(page.locator("#results .card")).toHaveCount(0);
  await page.fill("#max-minutes", "29");
  await page.click("#search-btn");
  await expect(page.locator("#results .card")).toHaveCount(1);
});

test("(b) 60〜90 分は TIME_WINDOW の文言が表示される", async ({ page }) => {
  await openApp(page);
  await page.selectOption("#origin-preset", "kandabashi");
  await page.fill("#min-minutes", "60");
  await page.fill("#max-minutes", "90");
  await page.click("#search-btn");

  // Issue #57: 最近接の神田橋入口 tier は完全評価済みで合法周回（計画 ~28.6 分）を
  // 持つが、60 分下限に届かない。最近接入口優先の診断として、遠方入口へ縮退せず
  // TIME_WINDOW と証明済み minPlanSeconds を返す（座標検索のまま復帰導線が機能する）。
  await expect(page.locator("#status")).toContainText(
    "指定時間枠（60〜90 分）に収まる候補がありません",
  );
  const alternate = page.getByRole("button", { name: "別の入口・出口を指定する" });
  await expect(alternate).toBeVisible();
  await alternate.click();
  await expect(page.locator("#explicit-od-section")).toBeVisible();
  await expect(page.locator("#min-minutes")).toHaveValue("60");
  await expect(page.locator("#max-minutes")).toHaveValue("90");
});

test("(f) 240/240 の指定枠は TIME_WINDOW 診断と最小時間の復帰導線を出す", async ({ page }) => {
  await openApp(page);
  await page.selectOption("#origin-preset", "kandabashi");
  await page.fill("#min-minutes", "240");
  await page.fill("#max-minutes", "240");
  await page.click("#search-btn");

  // Issue #57: 最近接 tier が完全評価済みで合法周回を持つため、打切り（SEARCH_LIMIT）
  // ではなく TIME_WINDOW と証明済み minPlanSeconds を返す。上限は既に製品上限 240 分
  // なので「時間の上限を広げる」は出さず、最小時間を下げる導線だけを出す。
  await expect(page.locator("#status")).toContainText("候補がありません");
  await expect(page.locator("#results .card")).toHaveCount(0);
  await expect(
    page.locator("#recovery-actions button", { hasText: "時間の上限を広げる" }),
  ).toHaveCount(0);
  await expect(
    page.locator("#recovery-actions button", { hasText: "最小時間を" }),
  ).toBeVisible();
});

test("(c) graph.json 改ざんは ARTIFACT_MISMATCH で停止する", async ({ page }) => {
  await page.route(GRAPH_URL, async (route) => {
    const res = await route.fetch();
    const body = await res.text();
    await route.fulfill({ response: res, body: `${body}\n` }); // 末尾 1 バイト追加で sha/長さ不一致
  });
  await openApp(page);
  await page.click("#search-btn");
  await expect(page.locator("#status")).toContainText("ARTIFACT_MISMATCH");
});

test("(d) graph.json が 11 秒遅延すると TIMEOUT の文言が表示される", async ({ page }) => {
  // 11 秒遅延はテスト終了時に afterEach の abort で解除できる形にする。
  const abort = new AbortController();
  delayAbort = abort;

  await page.route(GRAPH_URL, async () => {
    await delayUnlessAborted(11_000, abort.signal);
  });
  await openApp(page);
  await page.click("#search-btn");
  await expect(page.locator("#status")).toContainText(
    "探索が 10 秒を超えました。再度検索してください",
    { timeout: 20_000 },
  );
});

test("(e) 入力エラーを直して探索ボタンを 1 回押すと探索が始まる", async ({ page }) => {
  await openApp(page);
  await page.selectOption("#origin-preset", "kandabashi");
  await page.fill("#min-minutes", "15");
  await page.fill("#max-minutes", "60");

  // 緯度を不正にして探索 → エラー表示（探索は実行されない）。
  await page.fill("#lat", "999");
  await page.click("#search-btn");
  await expect(page.locator("#input-errors li")).toHaveCount(1);
  await expect(page.locator("#input-errors")).toContainText("緯度");
  await expect(page.locator("#status")).toHaveText("入力に誤りがあります");
  await expect(page.locator("#results .card")).toHaveCount(0);

  // 誤りのある欄へフォーカスが移り、aria-describedby はその欄のエラーだけを指す
  // （design-review-002 L1）。ol はリストのまま role="alert" の div に包まれている（L2）。
  await expect(page.locator("#lat")).toBeFocused();
  const latDescribedBy = await page.locator("#lat").getAttribute("aria-describedby");
  expect(latDescribedBy).toBe(await page.locator("#input-errors li").first().getAttribute("id"));
  await expect(page.locator("#lon")).not.toHaveAttribute("aria-describedby", /.+/);
  await expect(page.locator("#input-errors")).toHaveJSProperty("tagName", "OL");
  await expect(page.locator("#input-errors-alert")).toHaveAttribute("role", "alert");

  // 緯度を修正 → 探索ボタンを 1 回クリック。エラー一覧はボタンより下にあるため
  // 消えてもボタンが動かず、この 1 クリックで探索が始まる（design-review-002 N1）。
  await page.fill("#lat", "35.6896727");
  await page.click("#search-btn");

  await expect(page.locator("#results .card").first()).toBeVisible();
  await expect(page.locator("#results .card")).toHaveCount(1);
  await expect(page.locator("#input-errors li")).toHaveCount(0);
});
