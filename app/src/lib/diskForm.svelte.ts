// The Disk tab's form. Kept here, outside the tab, so switching tabs doesn't
// lose what the user typed; it starts over when another disk goes in.
import type { ConfigForm, ConfigView } from "./api";

interface FormState extends ConfigForm {
  /** Filter of the Steam library list. */
  search: string;
  /** The disk the form was filled for (its config hash, or its code). */
  diskKey: string | null;
}

export const form: FormState = $state({
  kind: "steam",
  name: "",
  appId: null,
  path: "",
  args: "",
  process: "",
  search: "",
  diskKey: null,
});

/** Fills the form from a disk's configuration, or empties it. */
export function fillForm(config: ConfigView | null, diskKey: string) {
  form.kind = config?.kind ?? "steam";
  form.name = config?.name ?? "";
  form.appId = config?.appId ?? null;
  form.path = config?.path ?? "";
  form.args = config?.argsLine ?? "";
  form.process = config?.process ?? "";
  form.search = "";
  form.diskKey = diskKey;
}

/** What the backend checks and writes. */
export function formValues(): ConfigForm {
  return {
    kind: form.kind,
    name: form.name,
    appId: form.appId,
    path: form.path,
    args: form.args,
    process: form.process,
  };
}
