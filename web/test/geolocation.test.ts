// geolocation ラッパのユニットテスト（fake Geolocation を注入。navigator は使わない）。
import { describe, expect, it, vi } from "vitest";
import { getCurrentPosition } from "../src/geolocation";

type SuccessCallback = (position: GeolocationPosition) => void;
type ErrorCallback = (error: GeolocationPositionError) => void;

/** 結果を制御できる fake Geolocation。呼び出し回数と引数を記録する。 */
function fakeGeolocation(
  behavior: { coords?: GeolocationCoordinates; error?: { code: number; message?: string } },
): { geo: Geolocation; spy: ReturnType<typeof vi.fn> } {
  const spy = vi.fn((_success: SuccessCallback, _error?: ErrorCallback) => {
    if (behavior.error) {
      _error?.(behavior.error as GeolocationPositionError);
    } else {
      _success?.({ coords: behavior.coords } as GeolocationPosition);
    }
  });
  return { geo: { getCurrentPosition: spy } as unknown as Geolocation, spy };
}

describe("getCurrentPosition", () => {
  it("成功時は lat/lon を返す", async () => {
    const { geo } = fakeGeolocation({
      coords: { latitude: 35.6896727, longitude: 139.7644248 } as GeolocationCoordinates,
    });
    await expect(getCurrentPosition(geo)).resolves.toEqual({
      lat: 35.6896727,
      lon: 139.7644248,
    });
  });

  it("権限拒否（code 1）は { code: 1 } で reject する", async () => {
    const { geo } = fakeGeolocation({ error: { code: 1, message: "denied" } });
    await expect(getCurrentPosition(geo)).rejects.toEqual({ code: 1 });
  });

  it("位置情報利用不可（code 2）は { code: 2 } で reject する", async () => {
    const { geo } = fakeGeolocation({ error: { code: 2, message: "unavailable" } });
    await expect(getCurrentPosition(geo)).rejects.toEqual({ code: 2 });
  });

  it("タイムアウト（code 3）は { code: 3 } で reject する", async () => {
    const { geo } = fakeGeolocation({ error: { code: 3, message: "timeout" } });
    await expect(getCurrentPosition(geo)).rejects.toEqual({ code: 3 });
  });

  it("getCurrentPosition はちょうど 1 回だけ呼ばれる（権限の再要求をしない）", async () => {
    const { geo, spy } = fakeGeolocation({ error: { code: 1 } });
    await expect(getCurrentPosition(geo)).rejects.toEqual({ code: 1 });
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it("既定オプションは高精度・10 秒・キャッシュ禁止", async () => {
    const { geo, spy } = fakeGeolocation({
      coords: { latitude: 0, longitude: 0 } as GeolocationCoordinates,
    });
    await getCurrentPosition(geo);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0]?.[2]).toEqual({
      enableHighAccuracy: true,
      timeout: 10000,
      maximumAge: 0,
    });
  });

  it("オプション指定時は既定を上書きする", async () => {
    const { geo, spy } = fakeGeolocation({
      coords: { latitude: 0, longitude: 0 } as GeolocationCoordinates,
    });
    await getCurrentPosition(geo, { timeout: 5000 });
    expect(spy.mock.calls[0]?.[2]).toMatchObject({
      enableHighAccuracy: true,
      timeout: 5000,
      maximumAge: 0,
    });
  });
});
