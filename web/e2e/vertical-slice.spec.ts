// issue #12 完了条件の E2E 検証。
// (a) 神田橋プリセット → 15〜60 → 候補カードと Maps URL / window.open 引数
// (b) 60〜90 で TIME_WINDOW の文言
// (c) graph.json 改ざんで ARTIFACT_MISMATCH の文言
// (d) graph.json 11 秒遅延で TIMEOUT の文言
import { expect, test } from "@playwright/test";

const GRAPH_URL = "**/releases/c1-real-v1/graph.json";

async function openApp(page: import("@playwright/test").Page): Promise<void> {
  await page.goto("/");
  await expect(page.locator("#search-btn")).toBeVisible();
}

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
  await expect(page.locator("#results .card")).toHaveCount(2);

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

test("(b) 60〜90 分は TIME_WINDOW の文言が表示される", async ({ page }) => {
  await openApp(page);
  await page.selectOption("#origin-preset", "kandabashi");
  await page.fill("#min-minutes", "60");
  await page.fill("#max-minutes", "90");
  await page.click("#search-btn");

  await expect(page.locator("#status")).toContainText(
    "指定時間枠（60〜90 分）に収まる候補がありません",
  );
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
  await page.route(GRAPH_URL, async (route) => {
    const res = await route.fetch();
    const body = await res.text();
    await new Promise((resolve) => setTimeout(resolve, 11_000));
    await route.fulfill({ response: res, body });
  });
  await openApp(page);
  await page.click("#search-btn");
  await expect(page.locator("#status")).toContainText(
    "探索が 10 秒を超えました。再度検索してください",
    { timeout: 20_000 },
  );
});
