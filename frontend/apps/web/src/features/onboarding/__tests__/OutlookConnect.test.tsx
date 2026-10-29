// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react';

const { startOutlookDeviceCode, pollOutlookDeviceCode, getAccounts } = vi.hoisted(() => ({
  startOutlookDeviceCode: vi.fn(),
  pollOutlookDeviceCode: vi.fn(),
  getAccounts: vi.fn().mockResolvedValue([]),
}));

vi.mock('@emailibrium/api', () => ({
  startOutlookDeviceCode,
  pollOutlookDeviceCode,
  getAccounts,
}));

import { OutlookConnect } from '../OutlookConnect';

const FLOW = {
  flowId: 'flow-1',
  userCode: 'ABCD-EFGH',
  verificationUri: 'https://microsoft.com/devicelogin',
  expiresIn: 900,
  interval: 5,
};

const ACCOUNT = {
  id: 'acct-1',
  provider: 'imap',
  emailAddress: 'chris@outlook.com',
  archiveStrategy: 'delayed',
  syncDepth: '30d',
  labelPrefix: 'EM/',
  syncFrequency: 5,
  isActive: true,
  emailCount: 0,
};

async function startCodeFlow() {
  fireEvent.change(screen.getByLabelText(/outlook or microsoft 365 address/i), {
    target: { value: 'chris@outlook.com' },
  });
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: 'Get sign-in code' }));
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  getAccounts.mockResolvedValue([]);
  startOutlookDeviceCode.mockResolvedValue(FLOW);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('OutlookConnect device-code sign-in', () => {
  it('shows the code and Microsoft link, then connects when sign-in finishes', async () => {
    const onConnected = vi.fn();
    pollOutlookDeviceCode
      .mockResolvedValueOnce({ status: 'pending', interval: 5 })
      .mockResolvedValueOnce({ status: 'connected', account: ACCOUNT });
    render(<OutlookConnect onBack={vi.fn()} onConnected={onConnected} />);

    await startCodeFlow();

    expect(startOutlookDeviceCode).toHaveBeenCalledWith('chris@outlook.com');
    expect(screen.getByLabelText('Sign-in code').textContent).toBe('ABCD-EFGH');
    const link = screen.getByRole('link', { name: FLOW.verificationUri });
    expect(link.getAttribute('target')).toBe('_blank');
    expect(link.getAttribute('rel')).toContain('noopener');

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });

    expect(pollOutlookDeviceCode).toHaveBeenCalledTimes(2);
    expect(pollOutlookDeviceCode).toHaveBeenCalledWith('flow-1');
    expect(onConnected).toHaveBeenCalledWith(ACCOUNT);
  });

  it('polls no faster than the interval, and slows down when asked', async () => {
    pollOutlookDeviceCode
      .mockResolvedValueOnce({ status: 'slow_down', interval: 10 })
      .mockResolvedValue({ status: 'pending', interval: 10 });
    render(<OutlookConnect onBack={vi.fn()} onConnected={vi.fn()} />);
    await startCodeFlow();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(4999);
    });
    expect(pollOutlookDeviceCode).toHaveBeenCalledTimes(0);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(pollOutlookDeviceCode).toHaveBeenCalledTimes(1);

    // Interval is now 10 seconds.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(9999);
    });
    expect(pollOutlookDeviceCode).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(pollOutlookDeviceCode).toHaveBeenCalledTimes(2);
  });

  it('stops polling and offers a new code when the code expires', async () => {
    pollOutlookDeviceCode.mockResolvedValueOnce({ status: 'expired' });
    render(<OutlookConnect onBack={vi.fn()} onConnected={vi.fn()} />);
    await startCodeFlow();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });

    expect(screen.getByRole('alert').textContent).toMatch(/expired/i);
    expect(screen.getByRole('button', { name: 'Get a new code' })).toBeTruthy();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(pollOutlookDeviceCode).toHaveBeenCalledTimes(1);
  });

  it('rejects an invalid address without calling the server', async () => {
    render(<OutlookConnect onBack={vi.fn()} onConnected={vi.fn()} />);
    fireEvent.change(screen.getByLabelText(/outlook or microsoft 365 address/i), {
      target: { value: 'not-an-address' },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Get sign-in code' }));
    });

    expect(startOutlookDeviceCode).not.toHaveBeenCalled();
    expect(screen.getByRole('alert').textContent).toMatch(/valid email/i);
  });

  it('keeps the own-Azure-app redirect under Advanced', () => {
    render(<OutlookConnect onBack={vi.fn()} onConnected={vi.fn()} />);
    expect(screen.queryByRole('button', { name: /redirect/i })).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: /advanced/i }));

    expect(
      screen.getByRole('button', { name: /sign in with microsoft \(redirect\)/i }),
    ).toBeTruthy();
  });
});
