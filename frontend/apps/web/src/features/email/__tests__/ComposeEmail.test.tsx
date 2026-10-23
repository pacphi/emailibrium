// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { sendEmail } from '@emailibrium/api';
import { ComposeEmail } from '../ComposeEmail';

vi.mock('@emailibrium/api', () => ({
  sendEmail: vi.fn(),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function renderCompose(isOpen: boolean) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ComposeEmail
        isOpen={isOpen}
        onClose={vi.fn()}
        accounts={[{ id: 'acct-1', emailAddress: 'me@example.com', provider: 'gmail' }]}
      />
    </QueryClientProvider>,
  );
}

describe('ComposeEmail focus management', () => {
  it('renders nothing when closed', () => {
    const { container } = renderCompose(false);

    expect(container.querySelector('#compose-to')).toBeNull();
  });

  it('autofocuses the To field when opened, so keyboard focus stays inside the modal', () => {
    renderCompose(true);

    const toInput = document.getElementById('compose-to');
    expect(toInput).not.toBeNull();
    expect(document.activeElement).toBe(toInput);
  });
});

describe('ComposeEmail sender identity', () => {
  const alice = { id: 'alice', emailAddress: 'alice@example.test', provider: 'gmail' };
  const bob = { id: 'bob', emailAddress: 'bob@example.test', provider: 'gmail' };

  function changingAccounts(initial: (typeof alice)[]) {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const props = { isOpen: true, onClose: vi.fn() };
    const tree = (accounts: (typeof alice)[]) => (
      <QueryClientProvider client={client}>
        <ComposeEmail {...props} accounts={accounts} />
      </QueryClientProvider>
    );
    const view = render(tree(initial));
    fireEvent.change(screen.getByLabelText('To'), { target: { value: 'recipient@example.test' } });
    return (accounts: (typeof alice)[]) => view.rerender(tree(accounts));
  }

  it('keeps the sender selected after asynchronous accounts arrive and reorder', async () => {
    const updateAccounts = changingAccounts([]);
    updateAccounts([alice, bob]);
    await waitFor(() =>
      expect((screen.getByLabelText('From') as HTMLSelectElement).value).toBe('alice'),
    );
    updateAccounts([bob, alice]);
    expect((screen.getByLabelText('From') as HTMLSelectElement).value).toBe('alice');
  });

  it('requires a new sender choice when the selected account disappears', () => {
    const updateAccounts = changingAccounts([alice, bob]);
    fireEvent.change(screen.getByLabelText('From'), { target: { value: 'bob' } });
    updateAccounts([alice]);
    expect((screen.getByLabelText('Send email') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByLabelText('From') as HTMLSelectElement).value).toBe('');
    fireEvent.click(screen.getByLabelText('Send email'));
    expect(sendEmail).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText('From'), { target: { value: 'alice' } });
    expect((screen.getByLabelText('Send email') as HTMLButtonElement).disabled).toBe(false);
  });
});
