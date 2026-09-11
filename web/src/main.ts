// 第 1 段の最小骨格: Worker を生成して ready をコンソールに出す。
// 本格的な UI 描画は第 2 段で実装する。
const worker = new Worker(new URL("./worker/search-worker.ts", import.meta.url), {
  type: "module",
});

worker.onmessage = (event: MessageEvent) => {
  console.log("[worker]", event.data);
};
