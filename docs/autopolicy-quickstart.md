# Quick Start: AutoPolicy Autonomous Workflow

**Version:** v2.14-dev
**Feature:** Complete Zero-Trust Policy Automation
**Time Required:** 5-10 minutes (after 7-day learning period)

---

## TL;DR

```bash
# 1. Start Paqtra
./paqtra

# 2. Navigate to AutoPolicy tab (Tab 6)
#    → System automatically learns patterns (5min intervals)

# 3. After learning period (e.g., 7 days):
#    Press 'g' → Generate policies
#    Press 'v' → View policy details
#    Use ↑/↓   → Navigate between policies
#    Press 'a' → Apply selected policy
#    Press 'y' → Confirm application

# Done! Zero-trust policies deployed.
```

---

## Prerequisites

- Kubernetes cluster with Cilium CNI
- kubectl configured
- Paqtra installed
- Optional: 7 days of traffic for best results

---

## Step-by-Step Guide

### Step 1: Learning Phase (Automatic)

```
Open Paqtra TUI:
  $ ./paqtra

Navigate to AutoPolicy tab:
  Press Tab until you reach "AutoPolicy" (tab 6)

Wait and observe:
  • System automatically learns every 5 minutes
  • Display shows: "47 patterns | Confidence: 85%"
  • Or press 'u' to manually update

Recommended learning period: 7 days
  • Day 1: ~10 patterns, 45% confidence
  • Day 3: ~30 patterns, 68% confidence
  • Day 7: ~52 patterns, 85% confidence ✅
```

### Step 2: Generate Policies

```
When ready to generate:
  Press 'g'

Result:
  Status: "✅ Generated 5 policies → saved to ./policies/"
  Files created:
    - ./policies/auto-policy-web-to-api.yaml
    - ./policies/auto-policy-api-to-db.yaml
    - ./policies/auto-policy-web-to-cdn.yaml
    - ./policies/auto-policy-api-to-external.yaml
    - ./policies/auto-policy-test-to-api.yaml
```

### Step 3: Review Policies

```
Enter detail view:
  Press 'v'

You'll see:
  ┌──────────────────────────────────────────────┐
  │ 📄 Policy 1 of 5                             │
  │                                              │
  │ Name:       auto-policy-web-to-api           │
  │ Namespace:  prod                             │
  │ Confidence: 94.2%                            │
  │ Patterns:   4                                │
  │ Status:     📋 Not Applied                   │
  │ File:       ./policies/auto-policy-web...   │
  │                                              │
  │ [Full YAML displayed here]                   │
  └──────────────────────────────────────────────┘

Navigate:
  Press ↓ → Next policy
  Press ↑ → Previous policy

Review each policy:
  • Check confidence score (>90% = high confidence)
  • Verify YAML looks correct
  • Ensure namespaces and labels match expectations
```

### Step 4: Apply Policies

```
For each policy you want to apply:

1. Press 'a' (while viewing policy)
   → Confirmation prompt appears (RED footer)
   → Shows: "Apply policy 'X'? Press 'y' to confirm, 'n' to cancel"

2. Press 'y' to confirm (or 'n' to cancel)
   → kubectl apply executes
   → Status: "✅ Policy 'X' applied successfully"
   → Policy marked as "✅ APPLIED"

3. Continue to next policy:
   Press ↓ → Navigate to next
   Repeat steps 1-2

Tips:
  • Apply high-confidence policies first (>90%)
  • Skip low-confidence policies (<70%)
  • Can return later to apply remaining policies
```

### Step 5: Verify Deployment

```
Check applied status in TUI:
  • ✅ APPLIED = Successfully deployed
  • 📋 Not Applied = Not yet deployed

Outside TUI (optional):
  $ kubectl get cnp --all-namespaces
  $ kubectl describe cnp auto-policy-web-to-api -n prod

Monitor for issues:
  • Switch to RootCause tab (Tab 7)
  • Check for any policy denies
  • Adjust policies if needed
```

---

## Keyboard Reference

### AutoPolicy Tab Shortcuts

| Key | Action | Description |
|-----|--------|-------------|
| `u` | Update | Manual learning update (normally automatic) |
| `g` | Generate | Generate policies from learned patterns |
| `v` | View | Toggle policy detail view |
| `↑` | Previous | Navigate to previous policy (in detail view) |
| `↓` | Next | Navigate to next policy (in detail view) |
| `a` | Apply | Apply current policy (shows confirmation) |
| `y` | Yes | Confirm policy application |
| `n` | No | Cancel policy application |
| `Esc` | Exit/Cancel | Exit detail view or cancel confirmation |
| `q` | Quit | Quit application |
| `Tab` | Next Tab | Navigate to next tab |

---

## Common Workflows

### Workflow 1: Fresh Deployment (New Cluster)

```
Day 1:     Deploy applications
           Start Paqtra
           Navigate to AutoPolicy tab

Day 2-7:   System learns automatically
           (No action required)

Day 7:     Check confidence: 85% ✅
           Press 'g' to generate
           Press 'v' to review
           Apply policies with 'a' + 'y'

Result:    Zero-trust policies deployed from real traffic
```

### Workflow 2: Update Existing Policies

```
Current:   Policies deployed from previous generation

Changes:   New service added to production

Action:    Let system observe new traffic (1-2 days)
           Press 'g' to regenerate policies
           Press 'v' to review changes
           Apply updated policies with 'a' + 'y'

Result:    Policies updated to include new service
```

### Workflow 3: Selective Application

```
Scenario:  5 policies generated, only want to apply 3

Action:    Press 'v' to view details

           Policy 1 (94% conf): Press 'a' + 'y' ✅
           Policy 2 (91% conf): Press 'a' + 'y' ✅
           Policy 3 (67% conf): Press ↓ (skip)
           Policy 4 (88% conf): Press 'a' + 'y' ✅
           Policy 5 (72% conf): Press Esc (skip)

Result:    3 high-confidence policies applied
           2 low-confidence policies left for review
```

---

## Troubleshooting

### "No policies generated"

**Cause:** Not enough observations (need 10+ per pattern)

**Solution:**
- Wait longer (more traffic)
- Lower threshold in config
- Press 'u' to manually update learning

### "kubectl apply failed: namespace not found"

**Cause:** Target namespace doesn't exist

**Solution:**
```bash
kubectl create ns <namespace-name>
# Then retry application in TUI
```

### "Policy 'X' already applied"

**Cause:** Trying to apply same policy twice

**Solution:**
- This is expected (prevents duplicates)
- Policy is already in cluster
- Check with: `kubectl get cnp -n <namespace>`

### "Permission denied"

**Cause:** kubectl doesn't have permissions

**Solution:**
```bash
# Check kubectl access
kubectl auth can-i create ciliumnetworkpolicies

# Fix RBAC if needed
```

### "Low confidence scores"

**Cause:** Not enough observation time

**Solution:**
- Wait longer for more data
- Press 'u' periodically to update
- Apply only high-confidence policies initially

---

## Best Practices

### Learning Period

✅ **Recommended:**
- 7 days for production workloads
- 3 days minimum for stable traffic
- 1 day for development/testing

❌ **Avoid:**
- Generating after < 1 day
- Applying policies with < 70% confidence
- Rushing the learning phase

### Policy Application

✅ **Good Approach:**
- Review all policies before applying
- Apply high-confidence first (90%+)
- Test in staging before production
- Monitor for denies after application

❌ **Bad Approach:**
- Blindly applying all policies
- Skipping review step
- Applying without monitoring
- No rollback plan

### Maintenance

✅ **Regular Tasks:**
- Regenerate policies monthly
- Update for new services
- Review confidence scores
- Clean up obsolete policies

❌ **Don't:**
- Set and forget
- Never update policies
- Ignore low-confidence warnings
- Skip periodic review

---

## Example Session

```
$ ./paqtra

[Navigate to AutoPolicy tab]

Display shows:
  Learning Progress: 100% (7.0 days / 7 days)
  Patterns Learned:  52 unique flows
  Confidence:        High (87%)

[Press 'g']
Status: ✅ Generated 5 policies → saved to ./policies/

[Press 'v']
Viewing Policy 1 of 5
  Name:       auto-policy-web-to-api
  Confidence: 94.2%
  Status:     📋 Not Applied

[Press 'a']
Confirmation: Apply policy 'auto-policy-web-to-api'? y/n

[Press 'y']
Status: ✅ Policy 'auto-policy-web-to-api' applied successfully

[Press ↓]
Viewing Policy 2 of 5
  Name:       auto-policy-api-to-db
  Confidence: 91.8%
  Status:     📋 Not Applied

[Press 'a', then 'y']
Status: ✅ Policy 'auto-policy-api-to-db' applied successfully

[Continue for remaining policies...]

[Press Esc to exit detail view]

Done! Zero-trust policies deployed.
```

---

## Next Steps

After deploying policies:

1. **Monitor** - Check RootCause tab for any denies
2. **Verify** - Test applications still work correctly
3. **Audit** - Review kubectl get cnp --all-namespaces
4. **Document** - Save policy files to git repository
5. **Iterate** - Regenerate monthly or when services change

---

## Quick Reference

```
Learning:     Automatic (every 5min) or press 'u'
Generate:     Press 'g' after learning period
Review:       Press 'v' to view details
Navigate:     Press ↑/↓ between policies
Apply:        Press 'a' then 'y' to confirm
Exit:         Press Esc to return
Quit:         Press 'q' to quit application
```

---

## Support

- **Documentation:** See POLICY_APPLICATION_COMPLETE.md
- **Detailed Guide:** See AUTONOMOUS_WORKFLOW_COMPLETE.md
- **Issues:** Report at project repository
- **Questions:** Check documentation or ask team

---

**Last Updated:** 2026-02-06
**Version:** v2.14-dev
**Status:** Production Ready

🚀 Happy zero-trust policy deployment!
