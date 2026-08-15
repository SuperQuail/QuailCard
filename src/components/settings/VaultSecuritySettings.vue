<script setup lang="ts">
import { ref } from "vue";
import type { VaultStatus } from "../../domain/types";

/** 安全设置：保险库密码保护。表单校验本地完成，启用动作与状态由父层上抛。 */
defineProps<{
  vaultStatus: VaultStatus | null;
}>();

const emit = defineEmits<{
  "set-vault-password": [password: string];
}>();

const passwordFormOpen = ref(false);
const vaultPassword = ref("");
const vaultPasswordConfirm = ref("");
const passwordMessage = ref("");

/** 提交保险库密码：长度与一致性校验通过后上抛。 */
function applyVaultPassword(): void {
  if (vaultPassword.value.length < 8) {
    passwordMessage.value = "密码至少需要 8 位";
    return;
  }
  if (vaultPassword.value !== vaultPasswordConfirm.value) {
    passwordMessage.value = "两次输入的密码不一致";
    return;
  }
  passwordMessage.value = "";
  emit("set-vault-password", vaultPassword.value);
  passwordFormOpen.value = false;
  vaultPassword.value = "";
  vaultPasswordConfirm.value = "";
}
</script>

<template>
  <h2 class="text-[16px] font-semibold">安全</h2>
  <section class="mt-6">
    <p class="text-[11px] font-semibold tracking-wide text-ink-3 uppercase">凭据</p>
    <div class="setting-row">
      <div class="min-w-0">
        <p class="flex items-center gap-2 text-[12px] font-medium">
          密码保护
          <span class="rounded-full bg-marker px-2 py-0.5 text-[10px] font-semibold text-accent-strong">
            {{ vaultStatus?.protectionMode === "password" ? "已启用" : "未启用" }}
          </span>
        </p>
        <p class="mt-0.5 text-[11px] leading-5 text-ink-3">凭据以加密形式保存在本地；重启后需密码解锁，密码不保存。</p>
      </div>
      <button type="button" class="ghost-btn shrink-0 border border-hairline" @click="passwordFormOpen = !passwordFormOpen">
        {{ passwordFormOpen ? "收起" : "设置密码" }}
      </button>
    </div>
    <div v-if="passwordFormOpen" class="flex max-w-[300px] flex-col gap-2 pt-3">
      <input v-model="vaultPassword" type="password" class="field-input" placeholder="至少 8 位" />
      <input v-model="vaultPasswordConfirm" type="password" class="field-input" placeholder="再输入一次" @keyup.enter="applyVaultPassword" />
      <button type="button" class="primary-btn self-start" :disabled="vaultPassword.length < 8 || vaultPassword !== vaultPasswordConfirm" @click="applyVaultPassword">启用密码保护</button>
      <p v-if="passwordMessage" class="text-[11px] text-danger">{{ passwordMessage }}</p>
    </div>
  </section>
</template>
