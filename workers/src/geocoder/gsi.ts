import { Candidate } from "../types";
import { GeocoderAdapter } from "./adapter";

const GSI_ENDPOINT = "https://msearch.gsi.go.jp/address-search/AddressSearch";

interface GsiFeature {
  geometry?: {
    type?: string;
    coordinates?: [number, number] | number[];
  };
  properties?: {
    title?: string;
  };
}

export class GsiGeocoderAdapter implements GeocoderAdapter {
  async geocode(query: string, signal?: AbortSignal): Promise<Candidate[]> {
    const url = `${GSI_ENDPOINT}?q=${encodeURIComponent(query)}`;
    let response: Response;
    try {
      response = await fetch(url, { signal });
    } catch (err: unknown) {
      if (err instanceof Error && err.name === "AbortError") {
        throw err;
      }
      if (signal?.aborted) {
        const abortErr = new Error("Request aborted");
        abortErr.name = "AbortError";
        throw abortErr;
      }
      throw new Error("Upstream network failure");
    }

    if (!response.ok) {
      throw new Error("Upstream non-2xx status");
    }

    let data: unknown;
    try {
      data = await response.json();
    } catch {
      throw new Error("Upstream invalid JSON");
    }

    if (!Array.isArray(data)) {
      throw new Error("Upstream response is not an array");
    }

    const candidates: Candidate[] = [];
    for (const item of data as GsiFeature[]) {
      if (!item || typeof item !== "object") continue;
      const title = item.properties?.title;
      const coords = item.geometry?.coordinates;
      if (typeof title !== "string" || !Array.isArray(coords) || coords.length < 2) {
        continue;
      }

      const lon = coords[0];
      const lat = coords[1];

      if (
        typeof lat !== "number" ||
        typeof lon !== "number" ||
        !Number.isFinite(lat) ||
        !Number.isFinite(lon) ||
        lat < -90 ||
        lat > 90 ||
        lon < -180 ||
        lon > 180
      ) {
        continue;
      }

      candidates.push({
        label: title,
        lat,
        lon,
      });

      if (candidates.length >= 5) {
        break;
      }
    }

    return candidates;
  }
}
