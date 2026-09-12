declare global {
  namespace Cloudflare {
    interface Env {
      ARTIFACTS_BUCKET: R2Bucket;
      IP_RATE_LIMITER: RateLimit;
      GLOBAL_RATE_LIMITER: RateLimit;
      ALLOWED_RELEASES: string;
      GEOCODER_API_KEY?: string;
    }
  }
}

export type Env = Cloudflare.Env;

export interface Candidate {
  label: string;
  lat: number;
  lon: number;
}

export interface GeocodeRequest {
  query: string;
}

export interface GeocodeResponse {
  candidates: Candidate[];
}

export interface ErrorDetail {
  code: string;
  retryable: boolean;
}

export interface ErrorResponse {
  error: ErrorDetail;
}

export interface StructuredLog {
  event: string;
  status: number;
  durationMs: number;
  releaseId?: string;
  artifact?: string;
  candidateCount?: number;
  benchmarkKey?: string;
  benchmarkBytes?: number;
  errorCode?: string;
}

export interface HandlerLogMeta {
  releaseId?: string;
  artifact?: string;
  candidateCount?: number;
  benchmarkKey?: string;
  benchmarkBytes?: number;
  errorCode?: string;
}

export interface HandlerResult {
  response: Response;
  logMeta?: HandlerLogMeta;
}

