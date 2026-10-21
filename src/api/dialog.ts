import { open } from "@tauri-apps/plugin-dialog";
import { popover } from "./commands";

/** Opens the native folder picker. Resolves to `null` when the user cancels. */
export async function chooseProjectFolder(): Promise<string | null> {
  await popover.beginDialog();
  try {
    const selection = await open({
      directory: true,
      multiple: false,
      title: "Choose the project folder ThreadBack may collect evidence from",
    });
    return typeof selection === "string" ? selection : null;
  } finally {
    await popover.endDialog().catch(() => {});
  }
}
