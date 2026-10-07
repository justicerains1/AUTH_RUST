export function FieldError({ id, message }: { id: string; message: string | undefined }) {
  if (!message) return null;
  return <p id={id} className="field-error" role="alert"><span aria-hidden="true">！</span>{message}</p>;
}
