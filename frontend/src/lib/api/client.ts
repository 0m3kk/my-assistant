const TOKEN_KEY = "assistant_token"

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY)
}

export function setToken(token: string | null) {
  if (token) {
    localStorage.setItem(TOKEN_KEY, token)
  } else {
    localStorage.removeItem(TOKEN_KEY)
  }
}

export class ApiError extends Error {
  readonly status: number
  readonly title: string

  constructor(status: number, title: string, detail: string) {
    super(detail)
    this.status = status
    this.title = title
  }
}

async function request<T>(
  path: string,
  init: RequestInit & { json?: unknown; form?: FormData } = {},
): Promise<T> {
  const headers = new Headers(init.headers)
  const token = getToken()
  if (token) {
    headers.set("Authorization", `Bearer ${token}`)
  }
  let body = init.body
  if (init.form !== undefined) {
    // FormData: let the browser set the multipart boundary header.
    body = init.form
  } else if (init.json !== undefined) {
    headers.set("Content-Type", "application/json")
    body = JSON.stringify(init.json)
  }
  const response = await fetch(`/api${path}`, { ...init, headers, body })
  if (response.status === 204) {
    return undefined as T
  }
  const contentType = response.headers.get("content-type") ?? ""
  if (!contentType.includes("application/json")) {
    if (!response.ok) {
      throw new ApiError(response.status, "Request failed", await response.text())
    }
    return undefined as T
  }
  const data = await response.json()
  if (!response.ok) {
    throw new ApiError(
      response.status,
      data.title ?? "Request failed",
      data.detail ?? JSON.stringify(data),
    )
  }
  return data as T
}

export const api = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, json?: unknown) => request<T>(path, { method: "POST", json }),
  put: <T>(path: string, json?: unknown) => request<T>(path, { method: "PUT", json }),
  patch: <T>(path: string, json?: unknown) => request<T>(path, { method: "PATCH", json }),
  delete: <T>(path: string) => request<T>(path, { method: "DELETE" }),
  upload: <T>(path: string, form: FormData) => request<T>(path, { method: "POST", form }),
}

/** Fetch a binary endpoint with the auth header and return its Blob. */
export async function fetchBlob(path: string): Promise<Blob> {
  const headers = new Headers()
  const token = getToken()
  if (token) {
    headers.set("Authorization", `Bearer ${token}`)
  }
  // Paths may arrive already prefixed (e.g. FileDto.url = /api/files/...).
  const normalized = path.startsWith("/api") ? path.slice("/api".length) : path
  const response = await fetch(`/api${normalized}`, { headers })
  if (!response.ok) {
    const text = await response.text()
    let title = "Request failed"
    let detail = text
    try {
      const data = JSON.parse(text)
      title = data.title ?? title
      detail = data.detail ?? text
    } catch {
      // not problem+json — keep raw text
    }
    throw new ApiError(response.status, title, detail)
  }
  return response.blob()
}
