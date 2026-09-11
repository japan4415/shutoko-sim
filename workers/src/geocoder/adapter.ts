import { Candidate } from "../types";

export interface GeocoderAdapter {
  geocode(query: string, signal?: AbortSignal): Promise<Candidate[]>;
}
