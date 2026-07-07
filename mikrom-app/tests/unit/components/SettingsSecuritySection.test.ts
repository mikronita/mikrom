import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import SettingsSecuritySection from "$lib/components/settings/SettingsSecuritySection.svelte";

const mocks = vi.hoisted(() => ({
  setupTotp: vi.fn(),
  verifyTotp: vi.fn(),
  disableTotp: vi.fn(),
  changePassword: vi.fn(),
  deleteAccount: vi.fn(),
  getUserProfile: vi.fn(),
  getToken: vi.fn(),
  goto: vi.fn(),
  logout: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
  loading: vi.fn(),
  dismiss: vi.fn(),
  qrCodeToDataURL: vi.fn().mockResolvedValue("data:image/png;base64,mocked-qr-code"),
}));

vi.mock("qrcode", () => ({
  default: { toDataURL: mocks.qrCodeToDataURL },
}));

vi.mock("$lib/api", () => ({
  setupTotp: mocks.setupTotp,
  verifyTotp: mocks.verifyTotp,
  disableTotp: mocks.disableTotp,
  changePassword: mocks.changePassword,
  deleteAccount: mocks.deleteAccount,
  getUserProfile: mocks.getUserProfile,
}));

vi.mock("$lib/components", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/components")>();
  const { default: MockModal } = await import("./ModalFixture.svelte");
  return {
    ...actual,
    Modal: MockModal,
  };
});

vi.mock("$lib/auth", () => ({
  getToken: mocks.getToken,
  logout: mocks.logout,
}));

vi.mock("$app/navigation", () => ({
  goto: mocks.goto,
}));

vi.mock("$lib/toast", () => ({
  toast: {
    success: mocks.success,
    error: mocks.error,
    loading: mocks.loading,
    dismiss: mocks.dismiss,
  },
}));

const defaultProfile = {
  id: "user-1",
  email: "user@example.com",
  role: "user",
  first_name: null,
  last_name: null,
  avatar_url: null,
  vpc_ipv6_prefix: "fd00::",
  totp_enabled: false,
};

const totpSetupResponse = {
  secret: "JBSWY3DPEHPK3PXP",
  otpauth_url: "otpauth://totp/Mikrom:user@example.com?secret=JBSWY3DPEHPK3PXP&issuer=Mikrom",
};

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getToken.mockReturnValue("test-token");
  mocks.getUserProfile.mockResolvedValue({ data: defaultProfile });
});

describe("SettingsSecuritySection", () => {
  it("shows 'Configure 2FA' button when totp is not enabled", () => {
    render(SettingsSecuritySection, { props: { profile: defaultProfile } });

    expect(screen.getByText("Configure 2FA")).toBeTruthy();
    expect(screen.getByText("Not enabled")).toBeTruthy();
  });

  it("shows 'Disable 2FA' button when totp is enabled", () => {
    render(SettingsSecuritySection, {
      props: { profile: { ...defaultProfile, totp_enabled: true } },
    });

    expect(screen.getByText("Disable 2FA")).toBeTruthy();
    expect(screen.getByText("Enabled")).toBeTruthy();
  });

  it("opens setup modal on 'Configure 2FA' click", async () => {
    mocks.setupTotp.mockResolvedValue({ data: totpSetupResponse });

    render(SettingsSecuritySection, { props: { profile: defaultProfile } });

    await fireEvent.click(screen.getByText("Configure 2FA"));

    await waitFor(() => {
      expect(mocks.setupTotp).toHaveBeenCalledOnce();
    });

    expect(screen.getByText("Or enter this secret manually:")).toBeTruthy();
    expect(screen.getByText(totpSetupResponse.secret)).toBeTruthy();
    expect(screen.getByText("Verify & enable")).toBeTruthy();
    expect(screen.getByText("Cancel")).toBeTruthy();
  });

  it("verifies totp and closes modal on valid code", async () => {
    mocks.setupTotp.mockResolvedValue({ data: totpSetupResponse });
    mocks.verifyTotp.mockResolvedValue({ success: true });

    render(SettingsSecuritySection, { props: { profile: defaultProfile } });

    await fireEvent.click(screen.getByText("Configure 2FA"));

    await waitFor(() => {
      expect(screen.getByText("Verify & enable")).toBeTruthy();
    });

    const input = screen.getByPlaceholderText("000000");
    await fireEvent.input(input, { target: { value: "123456" } });

    await fireEvent.click(screen.getByText("Verify & enable"));

    await waitFor(() => {
      expect(mocks.verifyTotp).toHaveBeenCalledWith("test-token", { code: "123456" });
    });

    expect(mocks.success).toHaveBeenCalledWith("Two-factor authentication enabled");
  });

  it("shows error toast on invalid totp code", async () => {
    mocks.setupTotp.mockResolvedValue({ data: totpSetupResponse });
    mocks.verifyTotp.mockResolvedValue({ success: false, error: "Invalid 2FA code" });

    render(SettingsSecuritySection, { props: { profile: defaultProfile } });

    await fireEvent.click(screen.getByText("Configure 2FA"));

    await waitFor(() => {
      expect(screen.getByText("Verify & enable")).toBeTruthy();
    });

    const input = screen.getByPlaceholderText("000000");
    await fireEvent.input(input, { target: { value: "000000" } });

    await fireEvent.click(screen.getByText("Verify & enable"));

    await waitFor(() => {
      expect(mocks.error).toHaveBeenCalledWith("Invalid 2FA code");
    });
  });

  it("closes modal on cancel", async () => {
    mocks.setupTotp.mockResolvedValue({ data: totpSetupResponse });

    render(SettingsSecuritySection, { props: { profile: defaultProfile } });

    await fireEvent.click(screen.getByText("Configure 2FA"));

    await waitFor(() => {
      expect(screen.getByText("Cancel")).toBeTruthy();
    });

    await fireEvent.click(screen.getByText("Cancel"));

    expect(screen.queryByText("Or enter this secret manually:")).toBeNull();
  });

  it("disables 2FA on 'Disable 2FA' click", async () => {
    mocks.disableTotp.mockResolvedValue({ success: true });

    render(SettingsSecuritySection, {
      props: { profile: { ...defaultProfile, totp_enabled: true } },
    });

    await fireEvent.click(screen.getByText("Disable 2FA"));

    await waitFor(() => {
      expect(mocks.disableTotp).toHaveBeenCalledOnce();
    });

    expect(mocks.success).toHaveBeenCalledWith("Two-factor authentication disabled");
  });

  it("shows error toast on setup failure", async () => {
    mocks.setupTotp.mockResolvedValue({ error: "Failed to setup 2FA" });

    render(SettingsSecuritySection, { props: { profile: defaultProfile } });

    await fireEvent.click(screen.getByText("Configure 2FA"));

    await waitFor(() => {
      expect(mocks.error).toHaveBeenCalledWith("Failed to setup 2FA");
    });
  });
});
