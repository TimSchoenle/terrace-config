package de.timscho.config.loader;

/** What to do when one key is supplied by two mechanisms. */
public enum ShadowPolicy {

    /**
     * Refuse to load. <b>The default, and the reason this loader exists.</b>
     *
     * <p>Precedence would be the softer option, and it is the wrong one here. The failure this
     * prevents is a half-migrated deployment where a stale environment variable shadows a
     * mounted secret that has since been rotated — the service keeps working, with the old
     * credential, and the discrepancy surfaces during an incident rather than during a deploy.
     */
    REJECT,

    /**
     * Resolve by precedence: the environment first, then the secrets directory, then {@code
     * _FILE} indirection, each overwriting the last.
     *
     * <p>Exists so the loader is adoptable rather than because it is a good idea. Anyone
     * migrating from an ad hoc file-provider adapter has precedence semantics today and will not
     * switch to a loader that fails their boot on the first deploy.
     */
    LAST_WINS
}
