// issue #12 完了条件の E2E 検証。
// (a) 神田橋プリセット → 15〜60 → 候補カードと Maps URL / window.open 引数
// (b) 60〜90 で近接 tier のフォールバック候補（Issue #57）
// (c) graph.json 改ざんで ARTIFACT_MISMATCH の文言
// (d) graph.json 11 秒遅延で TIMEOUT の文言
// (e) 入力エラー修正後の再検索
// (f) 到達不能な指定枠の SEARCH_LIMIT 打切り文言（Issue #57）
import { expect, test } from "@playwright/test";

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
  await openApp(page);
  await page.selectOption("#origin-preset", "kandabashi");
  await page.fill("#min-minutes", "15");
  await page.fill("#max-minutes", "60");
  await page.click("#search-btn");

  const firstCard = page.locator("#results .card").first();
  await expect(firstCard).toBeVisible();
  await expect(firstCard).toContainText("神田橋入口");
  await expect(firstCard).toContainText("300");
  await expect(page.locator("#results .card")).toHaveCount(1);

  // 強調は所要時間の数値だけに当たり、但し書きは別要素の補助テキスト（design-review-002 C2）。
  await expect(firstCard.locator(".duration")).toHaveText(/^総所要時間: 約\d+分$/);
  await expect(firstCard.locator(".duration-note")).toHaveText(
    "アクセス・帰着を含み（概算）、休憩は含みません",
  );

  // 同じ但し書きを時間の入力欄にも併記し、両 input から aria-describedby で結ぶ
  // （design-review-002 D1 / 修正 6）。
  await expect(page.locator("#time-note")).toHaveText("アクセス・帰着を含み（概算）、休憩は含みません");
  await expect(page.locator("#min-minutes")).toHaveAttribute("aria-describedby", "time-note");
  await expect(page.locator("#max-minutes")).toHaveAttribute("aria-describedby", "time-note");

  const mapsUrl = await firstCard.getAttribute("data-maps-url");
  expect(mapsUrl, "data-maps-url 属性").not.toBeNull();
  expect(mapsUrl?.startsWith("https://www.google.com/maps/dir/?api=1")).toBe(true);

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
  await firstCard.locator("button").click();
  const calls = await page.evaluate(() => {
    const w = window as unknown as { __openCalls: string[][] };
    return w.__openCalls;
  });
  expect(calls).toHaveLength(1);
  expect(calls[0]?.[0]?.startsWith("https://www.google.com/maps/dir/?api=1")).toBe(true);
  expect(calls[0]?.[1]).toBe("_blank");
  expect(calls[0]?.[2]).toBe("noopener");
});

test("(b) 神田橋プリセット 60〜90 分は近接 tier のフォールバックで候補が返る", async ({ page }) => {
  await openApp(page);
  await page.selectOption("#origin-preset", "kandabashi");
  await page.fill("#min-minutes", "60");
  await page.fill("#max-minutes", "90");
  await page.click("#search-btn");

  // Issue #57: 検証済みの神田橋入口 tier では 60〜90 分に収まる周回が得られないため、
  // 次の近接 tier（c1-inner 神田橋入口、約 65 m）が動的 OD として候補化する。
  await expect(page.locator("#status")).toContainText("候補が 2 件見つかりました。");
  await expect(page.locator("#results .card")).toHaveCount(2);
});

test("(f) 到達不能な指定枠は SEARCH_LIMIT の打切り文言で fail-closed を示す", async ({ page }) => {
  await openApp(page);
  await page.selectOption("#origin-preset", "kandabashi");
  await page.fill("#min-minutes", "240");
  await page.fill("#max-minutes", "240");
  await page.click("#search-btn");

  // Issue #57: 完全評価済み no-candidate tier のフォールスルーが共有 Budget を使い切るため、
  // 遠方の Verified 入口へ縮退せず打切り（SEARCH_LIMIT）で停止する。
  await expect(page.locator("#status")).toContainText("探索が上限に達したため");
  await expect(page.locator("#status")).toContainText("打ち切られています");
  await expect(page.locator("#results .card")).toHaveCount(0);
  await expect(
    page.locator("#recovery-actions button", { hasText: "時間の上限を広げる" }),
  ).toHaveCount(0);
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

  await page.route(GRAPH_URL, async (route) => {
    const res = await route.fetch();
    const body = await res.text();
    await delayUnlessAborted(11_000, abort.signal);
    if (abort.signal.aborted) {
      // テストは TIMEOUT 文言を確認して終了済み。ページ破棄後に fulfill すると
      // 例外になるため、保留のまま route を破棄する（unrouteAll が ignoreErrors で処理）。
      return;
    }
    await route.fulfill({ response: res, body });
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
