use std::fmt::Debug;
use std::str::FromStr;

pub fn get_env_try<T: FromStr>(env: &'static str) -> Result<T, <T as FromStr>::Err>
where
    <T as FromStr>::Err: Debug,
    <T as FromStr>::Err: From<std::env::VarError>,
{
    let result = std::env::var(env)?.parse::<T>()?;
    Ok(result)
}

pub fn get_env<T: FromStr>(env: &'static str) -> T
where
    <T as FromStr>::Err: Debug,
{
    std::env::var(env)
        .unwrap_or_else(|_| panic!("Cannot get the {env} env variable"))
        .parse::<T>()
        .unwrap_or_else(|_| panic!("Unable to parse env variable {env}"))
}

pub fn get_env_default<T: FromStr>(env: &'static str, default: T) -> T
where
    <T as FromStr>::Err: Debug,
{
    let env_str = std::env::var(env);
    match env_str {
        Ok(env_str) => env_str.parse::<T>().unwrap_or_else(|_| {
            env_str
                .parse::<T>()
                .unwrap_or_else(|_| panic!("Unable to parse env variable {env}"))
        }),
        Err(_e) => default,
    }
}

pub fn get_env_opt(env: &'static str) -> Option<String> {
    std::env::var(env).ok()
}
