# Azure App Registration (Client ID) 创建指南

在使用 `od-cli` 之前，由于微软官方安全策略（限制第三方客户端调用微软内部第一方应用 ID，错误码 `AADSTS65002`），推荐您在 Azure Portal 中注册一个属于自己的免费 Azure 应用程序。

整个创建过程**完全免费**，且仅需 **1 ~ 2 分钟**。

---

## 📋 准备工作

- 一个微软账户（个人 Outlook/Hotmail/Live 账号，或企业/学校 Office 365 账号均可）。

---

## 🚀 步骤详解

### 步骤 1：进入 Azure 应用注册控制台

1. 在浏览器中打开并登录 [Azure Portal - 应用注册 (App registrations)](https://portal.azure.com/#blade/Microsoft_AAD_RegisteredApps/ApplicationsListBlade)。
2. 点击顶部的 **“+ 新注册” (New registration)** 按钮。

---

### 步骤 2：填写注册信息

进入注册页面后，按如下指引填写：

1. **名称 (Name)**：
   - 填写 `od-cli`（或任意您喜欢的名字，如 `My OneDrive CLI`）。
2. **受支持的账户类型 (Supported account types)**：
   - 必须选择 **第三项**：
     > **“任何组织目录(任何 Microsoft Entra ID 租户 - 多租户)中的账户和个人 Microsoft 账户(例如 Skype、Xbox)”**  
     > *(Accounts in any organizational directory and personal Microsoft accounts)*
3. **重定向 URI (Redirect URI)**（可选，但推荐此时填好）：
   - 左侧下拉菜单选择：**“公共客户端/移动和桌面应用程序 (Public client/native)”**
   - 右侧输入框填入：
     ```text
     https://login.microsoftonline.com/common/oauth2/nativeclient
     ```
4. 点击底部的 **“注册” (Register)** 按钮。

---

### 步骤 3：开启公共客户端流（关键步骤 ⚠️）

设备码（Device Code）登录必须开启公共客户端流，否则会报错：

1. 在刚刚创建的应用左侧导航栏中，点击 **“身份验证” (Authentication)**。
2. 往下拉找到 **“高级设置” (Advanced settings)**。
3. 找到 **“允许公共客户端流” (Allow public client flows)** -> **“启用以下移动和桌面流”**。
4. 将选项切换为 **“是” (Yes)**。
5. 点击顶部的 **“保存” (Save)** 按钮。

---

### 步骤 4：配置 Microsoft Graph API 权限

1. 在左侧导航栏中，点击 **“API 权限” (API permissions)**。
2. 点击 **“+ 添加权限” (Add a permission)**。
3. 在常用 Microsoft API 中选择 **“Microsoft Graph”**。
4. 选择 **“委托的权限” (Delegated permissions)**。
5. 在搜索框中分别搜索并勾选以下 **3 项权限**：
   - `Files.ReadWrite.All`（读写所有 OneDrive 文件）
   - `offline_access`（获取 Refresh Token，保持长期自动登录免反复输入验证码）
   - `User.Read`（读取用户头像与基本账号信息）
6. 点击底部的 **“添加权限” (Add permissions)** 确认。

> 💡 **提示**：如果使用的是个人微软账户（@outlook.com / @hotmail.com 等），首次登录时会自动弹出同意页面；如果是企业/组织账号，可点击旁边的“代表组织授予管理员同意”（如有管理员权限）。

---

### 步骤 5：获取 Client ID 并配置到 `od-cli`

1. 在左侧导航栏点击 **“概述” (Overview)**。
2. 在页面上方找到 **“应用程序(客户端) ID” (Application (client) ID)**，点击复制图标（格式类似于 `3f8a1234-abcd-4ef0-9123-1234567890ab`）。
3. 打开终端，运行以下命令将其配置为默认 Client ID：
   ```bash
   od-cli config set client_id <粘贴您复制的客户端ID>
   ```
4. （可选）如果您使用的是特定的企业租户，可以配置租户 ID（个人账户保持默认 `common` 即可）：
   ```bash
   od-cli config set tenant_id <您的_TENANT_ID>
   ```
5. 验证配置：
   ```bash
   od-cli config show
   ```

---

## 🔑 开始登录

配置完成后，直接运行登录命令：

```bash
od-cli auth login
```

终端将输出类似以下内容：
```text
==> Initiating Microsoft Azure Device Login...

=================== Microsoft Sign-In ===================
1. Open the URL in your browser: https://microsoft.com/devicelogin
2. Enter verification code:      XXXXXXXX
=========================================================
```
浏览器将自动弹出，输入上述代码并登录授权即可！

---

## ❓ 常见问题排查 (FAQ)

### 1. 报错 `AADSTS65002: Consent between first party application...`
- **原因**：使用了微软第一方预置的 Client ID，微软禁止第三方随意借用该 ID 申请 Graph 权限。
- **解决**：按照本文档在 Azure Portal 中注册应用并执行 `od-cli config set client_id <您的ID>`。

### 2. 报错 `AADSTS7000218: The request body must contain the following parameter: 'client_assertion' or 'client_secret'`
- **原因**：应用未开启公共客户端流（Public client flow）。
- **解决**：在应用后台 **身份验证 (Authentication)** -> **高级设置 (Advanced settings)** 中将 **“允许公共客户端流”** 设置为 **“是 (Yes)”** 并保存。

### 3. 报错 `AADSTS50011: The redirect URI specified in the request does not match...`
- **原因**：虽然设备码流主要依靠后台轮询，但某些租户要求重定向 URI 匹配。
- **解决**：在 **身份验证** 平台配置中添加 **公共客户端/移动和桌面应用程序**，URI 填入 `https://login.microsoftonline.com/common/oauth2/nativeclient`。
