import type { BootstrapData } from "../../domain/types";
import * as cardsDomain from "./cards";
import * as attachmentsDomain from "./attachments";
import * as notesDomain from "./notes";
import * as providersDomain from "./providers";
import {
  aiGradingEnabled,
  ensureSeeded,
  fontSize,
  mockActiveProviderId,
  mockProviders,
  notes,
  recentVaults,
  setAiGradingEnabled,
  setFontSize,
  setVaultLocked,
  setVaultPath,
  setVaultProtection,
  vaultLocked,
  vaultPath,
  vaultProtection,
} from "./state";

/**
 * 浏览器演示后端的命令分发入口。
 *
 * 契约：实现与后端 Tauri invoke 相同形状的 `call<T>(command, args)`，
 * 使 src/api/backend.ts 在无 Tauri 环境下可无缝回退到本模块。
 *
 * 重要：这里只做参数解析与委派，不写业务规则；各域逻辑见同目录
 * notes.ts / cards.ts / providers.ts / scheduling.ts，且皆为演示级简化。
 */
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  ensureSeeded();
  const now = Math.floor(Date.now() / 1000);
  switch (command) {
    case "get_bootstrap_data": {
      const data: BootstrapData = {
        notes: notesDomain.noteSummaries(),
        providers: [...mockProviders],
        activeProviderId: mockActiveProviderId,
        studyStats: cardsDomain.getStudyStats(now),
        fontSize: fontSize as BootstrapData["fontSize"],
        vaultPath,
        recentVaults: [...recentVaults],
        aiGradingEnabled,
      };
      return data as T;
    }
    case "open_vault": {
      const openedPath = String(args?.path ?? "");
      setVaultPath(openedPath);
      const index = recentVaults.indexOf(openedPath);
      if (index >= 0) {
        recentVaults.splice(index, 1);
      }
      recentVaults.unshift(openedPath);
      return openedPath as T;
    }
    case "get_recent_vaults":
      return [...recentVaults] as T;
    case "get_vault_path":
      return vaultPath as T;
    case "get_vault_config":
      return attachmentsDomain.getVaultConfig() as T;
    case "set_attachment_folder":
      return attachmentsDomain.setAttachmentFolder(String(args?.attachmentFolder ?? "")) as T;
    case "import_note_attachment":
      return attachmentsDomain.importNoteAttachment(args?.input as Parameters<typeof attachmentsDomain.importNoteAttachment>[0]) as T;
    case "read_note_attachment": {
      const input = args?.input as { notePath: string; source: string };
      return attachmentsDomain.readNoteAttachment(input.notePath, input.source) as T;
    }
    case "list_notes":
      return notesDomain.noteSummaries() as T;
    case "read_note":
      return notesDomain.readNote(String(args?.path ?? "")) as T;
    case "write_note": {
      const input = args?.input as { path: string; content: string };
      return notesDomain.writeNote(input.path, input.content) as T;
    }
    case "create_note_file":
      return notesDomain.createNoteFile(String(args?.folder ?? ""), String(args?.title ?? "")) as T;
    case "create_folder":
      // 演示环境不维护空文件夹：创建文件夹本身无副作用，返回 undefined。
      return undefined as T;
    case "rename_note_file":
      return notesDomain.renameNoteFile(String(args?.oldPath ?? ""), String(args?.newPath ?? "")) as T;
    case "delete_note_file":
      notesDomain.deleteNoteFile(String(args?.path ?? ""));
      return undefined as T;
    case "rename_folder":
      return notesDomain.renameFolder(String(args?.oldPath ?? ""), String(args?.newPath ?? "")) as T;
    case "delete_folder":
      notesDomain.deleteFolder(String(args?.path ?? ""));
      return undefined as T;
    case "save_card": {
      const input = args?.input as Parameters<typeof cardsDomain.saveCard>[0];
      return cardsDomain.saveCard(input) as T;
    }
    case "delete_card":
      cardsDomain.deleteCard(String(args?.cardId ?? ""));
      return undefined as T;
    case "list_note_cards":
      return cardsDomain.listNoteCards(String(args?.notePath ?? "")) as T;
    case "adopt_cards": {
      const input = args?.input as Parameters<typeof cardsDomain.adoptCards>[0];
      return cardsDomain.adoptCards(input) as T;
    }
    case "search":
      return notesDomain.search(String(args?.query ?? "")) as T;
    case "get_review_queue":
      return cardsDomain.getReviewQueue(
        (args?.notePath as string | null) ?? null,
        Boolean(args?.includeAll),
        now,
      ) as T;
    case "check_dictation": {
      const input = args?.input as { cardId: string; answer: string };
      return cardsDomain.checkDictation(input.cardId, input.answer) as T;
    }
    case "submit_review": {
      const input = args?.input as { cardId: string; rating: string };
      return cardsDomain.submitReview(input.cardId, input.rating) as T;
    }
    case "evaluate_answer": {
      const input = args?.input as { cardId: string; userAnswer: string };
      return cardsDomain.evaluateAnswerCommand(input.cardId, input.userAnswer) as T;
    }
    case "get_study_stats":
      return cardsDomain.getStudyStats(now) as T;
    case "set_font_size":
      // 演示环境允许就地改字号并回显，未持久化。
      setFontSize(String(args?.fontSize ?? "comfortable") as "compact" | "standard" | "comfortable");
      return fontSize as T;
    case "set_ai_grading_enabled":
      setAiGradingEnabled(Boolean(args?.enabled));
      return aiGradingEnabled as T;
    case "list_providers":
      return providersDomain.listProviders() as T;
    case "set_active_provider":
      providersDomain.setActiveProvider(String(args?.providerId ?? ""));
      return undefined as T;
    case "get_vault_status":
      return { protectionMode: vaultProtection, locked: vaultLocked } as T;
    case "set_vault_password":
      // 仅演示：就地标记为已设置密码并解锁，绝不能存储真实密码。
      setVaultProtection("password");
      setVaultLocked(false);
      return { protectionMode: vaultProtection, locked: false } as T;
    case "generate_cards": {
      const input = args?.input as Parameters<typeof cardsDomain.generateCards>[0];
      return cardsDomain.generateCards(input) as T;
    }
    case "sync_note_index":
      return Math.floor(Date.now() / 1000) as T;
    case "rescan_vault":
      return notes.size as T;
    case "save_provider": {
      const input = args?.input as Parameters<typeof providersDomain.saveProvider>[0];
      return providersDomain.saveProvider(input) as T;
    }
    case "delete_provider":
      providersDomain.deleteProvider(String(args?.providerId ?? ""));
      return undefined as T;
    case "test_provider": {
      const input = args?.input as Parameters<typeof providersDomain.testProvider>[0];
      return providersDomain.testProvider(input) as T;
    }
    case "start_openai_login":
      return providersDomain.startOpenAiLogin() as T;
    case "get_openai_login_status":
      return providersDomain.getOpenAiLoginStatus() as T;
    case "cancel_openai_login":
      return providersDomain.cancelOpenAiLogin() as T;
    case "logout_openai":
      return providersDomain.logoutOpenAi() as T;
    case "synthesize_speech":
      return providersDomain.synthesizeSpeech() as T;
    case "get_data_locations":
      // 演示环境无真实文件系统，返回与后端同形的占位路径。
      return {
        cardsDir: vaultPath ? `${vaultPath}/.quailcard` : null,
        configDir: "C:\\Users\\demo\\AppData\\Roaming\\QuailCard",
      } as T;
    case "reveal_data_folder":
      // 浏览器演示无法打开本地文件夹，静默成功即可。
      return undefined as T;
    default:
      throw new Error(`演示环境不支持命令：${command}`);
  }
}
