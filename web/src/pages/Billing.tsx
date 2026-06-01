import { useSubscription, useCheckout, usePortal } from '../lib/queries';
import type { Plan } from '../client/types.gen';

interface PlanCardProps {
  id: Plan;
  name: string;
  price: string;
  description: string;
  features: string[];
  currentPlan: string | null | undefined;
  onSubscribe: (plan: Plan) => void;
  isPending: boolean;
}

function PlanCard({
  id,
  name,
  price,
  description,
  features,
  currentPlan,
  onSubscribe,
  isPending,
}: PlanCardProps) {
  const isCurrent = currentPlan === id;

  return (
    <div className={`card plan-card${isCurrent ? ' plan-card--current' : ''}`}>
      {isCurrent && <span className="badge badge-active">Current plan</span>}
      <h3 className="plan-name">{name}</h3>
      <p className="plan-price">{price}<span className="plan-price-period">/mo</span></p>
      <p className="plan-description">{description}</p>
      <ul className="plan-features">
        {features.map((f) => (
          <li key={f} className="plan-feature-item">{f}</li>
        ))}
      </ul>
      <button
        className={`btn${isCurrent ? ' btn-secondary' : ' btn-primary'}`}
        type="button"
        disabled={isPending || isCurrent}
        onClick={() => onSubscribe(id)}
      >
        {isCurrent ? 'Subscribed' : 'Subscribe'}
      </button>
    </div>
  );
}

const PLANS: Omit<PlanCardProps, 'currentPlan' | 'onSubscribe' | 'isPending'>[] = [
  {
    id: 'founding',
    name: 'Founding',
    price: '$10',
    description: 'For the first ~100 users — grandfathered rate, locked in forever.',
    features: [
      'All Standard features',
      'Founding-member rate locked in',
      'Support the early build',
    ],
  },
  {
    id: 'standard',
    name: 'Standard',
    price: '$15',
    description: 'Everything you need to proxy AI traffic through your own handle.',
    features: [
      'One handle',
      'Multiple machines',
      'API key management',
      'Usage analytics',
    ],
  },
  {
    id: 'pro',
    name: 'Pro',
    price: '$25',
    description: 'Uncapped throughput, failover, multiple endpoints, and priority support.',
    features: [
      'Uncapped throughput',
      'Automatic failover',
      'Multiple endpoints',
      'Priority support',
    ],
  },
];

export function Billing() {
  const { data: sub, isPending: subLoading } = useSubscription();
  const checkoutMutation = useCheckout();
  const portalMutation = usePortal();

  function handleSubscribe(plan: Plan) {
    checkoutMutation.mutate(
      { plan },
      {
        onSuccess: ({ url }) => {
          window.location.href = url;
        },
      },
    );
  }

  function handleManage() {
    portalMutation.mutate(undefined, {
      onSuccess: ({ url }) => {
        window.location.href = url;
      },
    });
  }

  const mutationPending = checkoutMutation.isPending || portalMutation.isPending;

  return (
    <div className="billing-root">
      <h2 className="page-title">Billing</h2>

      {/* Current subscription status */}
      <section className="card billing-status">
        <h3 className="billing-status-heading">Current subscription</h3>
        {subLoading ? (
          <p className="billing-loading">Loading…</p>
        ) : sub ? (
          <dl className="billing-dl">
            <div className="billing-dl-row">
              <dt>Plan</dt>
              <dd>
                {sub.plan ?? 'None'}
                {sub.is_founding && (
                  <span className="badge badge-founding" title="Grandfathered founding rate">
                    Founding
                  </span>
                )}
              </dd>
            </div>
            <div className="billing-dl-row">
              <dt>Status</dt>
              <dd>
                {sub.status}
                <span className={`badge ${sub.active ? 'badge-active' : 'badge-inactive'}`}>
                  {sub.active ? 'Active' : 'Inactive'}
                </span>
              </dd>
            </div>
            {sub.current_period_end && (
              <div className="billing-dl-row">
                <dt>Renews</dt>
                <dd>{new Date(sub.current_period_end).toLocaleDateString()}</dd>
              </div>
            )}
          </dl>
        ) : (
          <p className="billing-no-sub">No active subscription.</p>
        )}

        {sub?.active && (
          <div className="billing-manage-row">
            <button
              className="btn btn-secondary"
              type="button"
              onClick={handleManage}
              disabled={portalMutation.isPending}
            >
              {portalMutation.isPending ? 'Opening portal…' : 'Manage subscription'}
            </button>
            {portalMutation.isError && (
              <p className="billing-error" role="alert">
                {portalMutation.error instanceof Error
                  ? portalMutation.error.message
                  : 'Could not open billing portal — please try again.'}
              </p>
            )}
          </div>
        )}
      </section>

      {/* Plan cards */}
      <section className="billing-plans">
        <h3 className="billing-plans-heading">Choose a plan</h3>
        {checkoutMutation.isError && (
          <p className="billing-error" role="alert">
            {checkoutMutation.error instanceof Error
              ? checkoutMutation.error.message
              : 'Checkout failed — please try again.'}
          </p>
        )}
        <div className="plan-cards-grid">
          {PLANS.map((plan) => (
            <PlanCard
              key={plan.id}
              {...plan}
              currentPlan={sub?.plan ?? null}
              onSubscribe={handleSubscribe}
              isPending={mutationPending}
            />
          ))}
        </div>
      </section>
    </div>
  );
}
