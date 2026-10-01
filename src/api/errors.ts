/** The serialized `DomainError` every failed command rejects with. */
export interface DomainError {
  code: string;
  message: string;
  retryable: boolean;
  field: string | null;
}

export function toDomainError(error: unknown): DomainError {
  if (typeof error === "object" && error !== null && "message" in error && "code" in error) {
    const candidate = error as Partial<DomainError>;
    return {
      code: String(candidate.code),
      message: String(candidate.message),
      retryable: Boolean(candidate.retryable),
      field: candidate.field ?? null,
    };
  }
  return {
    code: "INTERNAL",
    message: typeof error === "string" ? error : "Something went wrong. Please try again.",
    retryable: true,
    field: null,
  };
}
