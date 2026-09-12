// 現在地取得（GPS）の薄いラッパ。navigator.geolocation を注入可能にし、
// 位置情報 API を Promise 化する。権限拒否時は再要求せず即座に失敗させる。

/** 現在地取得の失敗理由。code は GeolocationPositionError.code に対応する。 */
export interface GeolocationFailure {
  code: number;
}

/** 既定の取得オプション。高精度・10 秒タイムアウト・キャッシュ禁止。 */
const DEFAULT_OPTIONS: PositionOptions = {
  enableHighAccuracy: true,
  timeout: 10000,
  maximumAge: 0,
};

/**
 * 現在地を 1 回だけ取得する。失敗時は `{ code }` で reject する。
 * 権限拒否（code 1）でも再試行・再要求は行わない。
 */
export function getCurrentPosition(
  geo: Geolocation = navigator.geolocation,
  options?: PositionOptions,
): Promise<{ lat: number; lon: number }> {
  return new Promise((resolve, reject) => {
    geo.getCurrentPosition(
      (position) => {
        resolve({ lat: position.coords.latitude, lon: position.coords.longitude });
      },
      (error) => {
        reject({ code: error.code } satisfies GeolocationFailure);
      },
      { ...DEFAULT_OPTIONS, ...options },
    );
  });
}
