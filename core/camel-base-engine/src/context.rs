//! The CamelContext of the engine (Java `DefaultCamelContext` / `AbstractCamelContext`).

use crate::producer_template::ProducerTemplate;
use crate::route::{DefaultRoute, RouteSpec};
use camel_api::context::CamelContext;
use camel_api::endpoint::{SharedComponent, SharedEndpoint};
use camel_api::enums::ServiceStatus;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::route::Route;
use camel_api::service::{ServiceState, StatefulService, SuspendableService};
use camel_api::spi::language::Language;
use camel_api::spi::log_listener::LogListener;
use camel_api::spi::properties::PropertiesComponent;
use camel_api::spi::registry::{lookup_bean, Registry};
use camel_api::spi::uuid::{DefaultUuidGenerator, UuidGenerator};
use camel_base::properties::DefaultPropertiesComponent;
use camel_support::component_support;
use camel_support::simple_registry::SimpleRegistry;
use camel_util::uri_support::normalize_uri;
use indexmap::IndexMap;
use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};

/// Creates a component for a uri scheme on first use.
pub type ComponentFactory = Arc<dyn Fn() -> SharedComponent + Send + Sync>;

/// Creates a language on first use.
pub type LanguageFactory = Arc<dyn Fn() -> Arc<dyn Language> + Send + Sync>;

static CONTEXT_COUNTER: AtomicU32 = AtomicU32::new(0);

/// The CamelContext of the routing engine.
///
/// Components are added with [`add_component`](Self::add_component), or created on first use
/// from a [`ComponentFactory`] registered for their scheme (Rust has no classpath scanning, so
/// the component crates are registered explicitly, usually by `camel-main`). Components can
/// also be bound in the [registry](CamelContext::registry) as an `Arc<dyn Component>`.
///
/// The context is always held in an `Arc` (see [`DefaultCamelContext::new`]), as endpoints and
/// exchanges refer back to it.
pub struct DefaultCamelContext {
    name: String,
    this: Weak<DefaultCamelContext>,
    uuid: Arc<dyn UuidGenerator>,
    registry: Arc<SimpleRegistry>,
    components: RwLock<IndexMap<String, SharedComponent>>,
    started_components: Mutex<HashSet<String>>,
    component_factories: RwLock<HashMap<String, ComponentFactory>>,
    languages: RwLock<HashMap<String, Arc<dyn Language>>>,
    language_factories: RwLock<HashMap<String, LanguageFactory>>,
    endpoints: RwLock<IndexMap<String, SharedEndpoint>>,
    routes: RwLock<Vec<Arc<DefaultRoute>>>,
    route_counter: AtomicU32,
    log_listeners: RwLock<Vec<Arc<dyn LogListener>>>,
    message_history: AtomicBool,
    plugins: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
    properties: Arc<DefaultPropertiesComponent>,
    state: ServiceState,
}

impl DefaultCamelContext {
    /// Creates a context named `camel-1`, `camel-2`, ... (as in Java).
    pub fn new() -> Arc<Self> {
        let n = CONTEXT_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
        Self::with_name(&format!("camel-{n}"))
    }

    /// Creates a context with the name.
    pub fn with_name(name: &str) -> Arc<Self> {
        Arc::new_cyclic(|this| DefaultCamelContext {
            name: name.to_string(),
            this: this.clone(),
            uuid: Arc::new(DefaultUuidGenerator::new()),
            registry: Arc::new(SimpleRegistry::new()),
            components: RwLock::new(IndexMap::new()),
            started_components: Mutex::new(HashSet::new()),
            component_factories: RwLock::new(HashMap::new()),
            languages: RwLock::new(HashMap::new()),
            language_factories: RwLock::new(HashMap::new()),
            endpoints: RwLock::new(IndexMap::new()),
            routes: RwLock::new(Vec::new()),
            route_counter: AtomicU32::new(0),
            log_listeners: RwLock::new(Vec::new()),
            message_history: AtomicBool::new(false),
            plugins: RwLock::new(HashMap::new()),
            properties: {
                let pc = Arc::new(DefaultPropertiesComponent::new());
                let weak: Weak<dyn CamelContext> = this.clone();
                pc.set_camel_context(weak);
                pc
            },
            state: ServiceState::new(),
        })
    }

    /// This context as a shared `CamelContext` (what endpoints and exchanges refer to).
    pub fn as_shared(&self) -> Arc<dyn CamelContext> {
        self.this
            .upgrade()
            .expect("the DefaultCamelContext is held in an Arc")
    }

    /// The bean registry.
    pub fn simple_registry(&self) -> &Arc<SimpleRegistry> {
        &self.registry
    }

    /// Sets whether message history is enabled.
    pub fn set_message_history(&self, enabled: bool) {
        self.message_history.store(enabled, Ordering::Relaxed);
    }

    /// The lifecycle status of the context.
    pub fn status(&self) -> ServiceStatus {
        self.state.status()
    }

    /// Adds the component for the scheme.
    ///
    /// # Errors
    ///
    /// Returns an `IllegalArgumentException` if a component with the name already exists
    /// (as in Java).
    pub fn add_component(&self, name: &str, component: SharedComponent) -> CamelResult<()> {
        let mut components = self.components.write().unwrap();
        if components.contains_key(name) {
            return Err(CamelError::illegal_argument(format!(
                "Cannot add component as its already previously added: {name}"
            )));
        }
        components.insert(name.to_string(), component);
        Ok(())
    }

    /// Registers a factory creating the component for the scheme on first use.
    pub fn register_component_factory(&self, name: &str, factory: ComponentFactory) {
        self.component_factories
            .write()
            .unwrap()
            .insert(name.to_string(), factory);
    }

    /// Adds a listener of the log messages (Java `addLogListener`).
    pub fn add_log_listener(&self, listener: Arc<dyn LogListener>) {
        self.log_listeners.write().unwrap().push(listener);
    }

    /// The properties component resolving the property placeholders (Java
    /// `getPropertiesComponent`), to configure its properties and locations.
    pub fn properties(&self) -> &Arc<DefaultPropertiesComponent> {
        &self.properties
    }

    /// Adds the plugin of the type `T` (Java `CamelContextExtension.addContextPlugin`),
    /// replacing an existing one; read with [`camel_api::context::context_plugin`].
    pub fn add_plugin<T: Any + Send + Sync>(&self, plugin: T) {
        self.plugins
            .write()
            .unwrap()
            .insert(TypeId::of::<T>(), Arc::new(plugin));
    }

    /// Adds the language with the name (replacing an existing one).
    pub fn add_language(&self, name: &str, language: Arc<dyn Language>) {
        self.languages
            .write()
            .unwrap()
            .insert(name.to_string(), language);
    }

    /// Registers a factory creating the language on first use.
    pub fn register_language_factory(&self, name: &str, factory: LanguageFactory) {
        self.language_factories
            .write()
            .unwrap()
            .insert(name.to_string(), factory);
    }

    /// The names of the languages in use.
    pub fn language_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.languages.read().unwrap().keys().cloned().collect();
        names.sort();
        names
    }

    /// The component for the scheme: an added one, one bound in the registry, or one created
    /// with a registered factory (Java `getComponent(name)`).
    pub fn component(&self, name: &str) -> Option<SharedComponent> {
        if let Some(c) = self.components.read().unwrap().get(name) {
            return Some(c.clone());
        }
        let created = lookup_bean::<SharedComponent>(self.registry.as_ref(), name)
            .map(|c| (*c).clone())
            .or_else(|| {
                self.component_factories
                    .read()
                    .unwrap()
                    .get(name)
                    .map(|f| f())
            })?;
        let mut components = self.components.write().unwrap();
        Some(
            components
                .entry(name.to_string())
                .or_insert(created)
                .clone(),
        )
    }

    /// The component for the scheme, as its concrete type (Java `getComponent(name, type)`).
    pub fn component_as<T: Any>(&self, name: &str) -> Option<SharedComponent> {
        self.component(name).filter(|c| c.as_any().is::<T>())
    }

    /// The names of the components in use.
    pub fn component_names(&self) -> Vec<String> {
        self.components.read().unwrap().keys().cloned().collect()
    }

    /// Removes the component.
    pub fn remove_component(&self, name: &str) -> Option<SharedComponent> {
        self.started_components.lock().unwrap().remove(name);
        self.components.write().unwrap().shift_remove(name)
    }

    /// The endpoints created so far.
    pub fn endpoints(&self) -> Vec<SharedEndpoint> {
        self.endpoints.read().unwrap().values().cloned().collect()
    }

    /// The endpoint for the uri if it was already created (Java `hasEndpoint`).
    pub fn has_endpoint(&self, uri: &str) -> Option<SharedEndpoint> {
        let key = normalize_uri(uri).ok()?;
        self.endpoints.read().unwrap().get(&key).cloned()
    }

    fn resolve_endpoint(&self, uri: &str) -> CamelResult<SharedEndpoint> {
        if uri.trim().is_empty() {
            return Err(CamelError::no_such_endpoint(uri));
        }
        // the property placeholders of the uri (Java getEndpoint)
        let resolved =
            camel_support::endpoint_helper::resolve_endpoint_uri_property_placeholders(self, uri)?;
        let uri = resolved.as_str();
        let normalized = normalize_uri(uri).map_err(|e| {
            CamelError::resolve_endpoint_failed(uri, camel_api::error::box_error(e))
        })?;
        if let Some(e) = self.endpoints.read().unwrap().get(&normalized) {
            return Ok(e.clone());
        }
        let scheme = camel_util::string_helper::before(&normalized, ":")
            .ok_or_else(|| CamelError::no_such_endpoint(uri))?;
        let component = self
            .component(&scheme)
            .ok_or_else(|| CamelError::no_such_endpoint(uri))?;
        let target = if component.use_raw_uri() {
            uri
        } else {
            normalized.as_str()
        };
        let endpoint =
            component_support::create_endpoint(component.as_ref(), &self.as_shared(), target)
                .map_err(|e| {
                    if e.is(ErrorKind::ResolveEndpointFailed) || e.is(ErrorKind::NoSuchEndpoint) {
                        e
                    } else {
                        CamelError::resolve_endpoint_failed(uri, e)
                    }
                })?;
        if endpoint.is_singleton() {
            let mut endpoints = self.endpoints.write().unwrap();
            return Ok(endpoints.entry(normalized).or_insert(endpoint).clone());
        }
        Ok(endpoint)
    }

    /// The endpoint for the uri, as its concrete type (Java `getEndpoint(uri, type)`).
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint cannot be resolved, or an `IllegalArgumentException`
    /// if it is of another type.
    pub fn endpoint_as<T: Any>(&self, uri: &str) -> CamelResult<SharedEndpoint> {
        let e = self.resolve_endpoint(uri)?;
        if e.as_any().is::<T>() {
            Ok(e)
        } else {
            Err(CamelError::illegal_argument(format!(
                "The endpoint is not of type: {} but is: {}",
                std::any::type_name::<T>(),
                e.endpoint_uri()
            )))
        }
    }

    async fn start_components(&self) -> CamelResult<()> {
        let components: Vec<(String, SharedComponent)> = self
            .components
            .read()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (name, c) in components {
            if self.started_components.lock().unwrap().insert(name.clone()) {
                c.init()?;
                c.start()
                    .await
                    .map_err(|e| CamelError::failed_to_start_component(&name, &e.to_string(), e))?;
            }
        }
        Ok(())
    }

    /// Adds a route; it is started right away if the context is started and the route is
    /// auto started (as in Java `addRoutes`).
    ///
    /// # Returns
    ///
    /// The route id.
    ///
    /// # Errors
    ///
    /// Returns a `FailedToCreateRouteException` if the endpoint cannot be resolved, or a
    /// `FailedToStartRouteException` if the id is a duplicate or the route fails to start.
    pub async fn add_route(&self, spec: RouteSpec) -> CamelResult<String> {
        let (id, custom) = match &spec.id {
            Some(id) => (id.clone(), spec.custom_id),
            None => (self.generate_route_id(), false),
        };
        if self.routes.read().unwrap().iter().any(|r| r.id() == id) {
            return Err(CamelError::failed_to_start_route(
                &id,
                &format!("duplicate id detected: {id}. Please correct ids to be unique among all your routes."),
            ));
        }
        let from = spec.from_uri.clone();
        let endpoint = self.resolve_endpoint(&from).map_err(|e| {
            CamelError::failed_to_create_route(
                &id,
                spec.source_location.as_deref(),
                &format!("From[{from}]"),
                e,
            )
        })?;
        let route = Arc::new(DefaultRoute::new(id.clone(), custom, endpoint, spec));
        self.routes.write().unwrap().push(route.clone());
        if self.state.status().is_started() && route.is_auto_startup() {
            self.start_components().await?;
            camel_api::service::Service::start(route.as_ref()).await?;
        }
        Ok(id)
    }

    /// Generates a route id: `route1`, `route2`, ... (Java `NodeIdFactory` for routes).
    pub fn generate_route_id(&self) -> String {
        format!(
            "route{}",
            self.route_counter.fetch_add(1, Ordering::SeqCst) + 1
        )
    }

    /// The routes.
    pub fn routes(&self) -> Vec<Arc<DefaultRoute>> {
        self.routes.read().unwrap().clone()
    }

    /// The route with the id.
    pub fn route(&self, id: &str) -> Option<Arc<DefaultRoute>> {
        self.routes
            .read()
            .unwrap()
            .iter()
            .find(|r| r.id() == id)
            .cloned()
    }

    fn mandatory_route(&self, id: &str) -> CamelResult<Arc<DefaultRoute>> {
        self.route(id)
            .ok_or_else(|| CamelError::illegal_argument(format!("Cannot find route with id: {id}")))
    }

    /// Starts the route (Java `RouteController.startRoute`).
    ///
    /// # Errors
    ///
    /// Returns an error if there is no such route or it fails to start.
    pub async fn start_route(&self, id: &str) -> CamelResult<()> {
        let route = self.mandatory_route(id)?;
        self.start_components().await?;
        camel_api::service::Service::start(route.as_ref()).await
    }

    /// Stops the route (Java `RouteController.stopRoute`).
    ///
    /// # Errors
    ///
    /// Returns an error if there is no such route or it fails to stop.
    pub async fn stop_route(&self, id: &str) -> CamelResult<()> {
        camel_api::service::Service::stop(self.mandatory_route(id)?.as_ref()).await
    }

    /// Suspends the route (Java `RouteController.suspendRoute`).
    ///
    /// # Errors
    ///
    /// Returns an error if there is no such route or it fails to suspend.
    pub async fn suspend_route(&self, id: &str) -> CamelResult<()> {
        self.mandatory_route(id)?.suspend().await
    }

    /// Resumes the suspended route (Java `RouteController.resumeRoute`).
    ///
    /// # Errors
    ///
    /// Returns an error if there is no such route or it fails to resume.
    pub async fn resume_route(&self, id: &str) -> CamelResult<()> {
        self.mandatory_route(id)?.resume().await
    }

    /// Removes the stopped route.
    ///
    /// # Returns
    ///
    /// Whether the route was removed (a route that is not stopped is not removed).
    pub fn remove_route(&self, id: &str) -> bool {
        let mut routes = self.routes.write().unwrap();
        match routes.iter().position(|r| r.id() == id) {
            Some(i) if !routes[i].status().is_started() && !routes[i].status().is_suspended() => {
                routes.remove(i);
                true
            }
            _ => false,
        }
    }

    /// The status of the route.
    pub fn route_status(&self, id: &str) -> Option<ServiceStatus> {
        self.route(id).map(|r| r.status())
    }

    /// Starts the context: the components, then the auto started routes in startup order.
    ///
    /// # Errors
    ///
    /// Returns the first failure (then the routes already started are stopped again).
    pub async fn start(&self) -> CamelResult<()> {
        self.state
            .start(|| async {
                log::info!(
                    "Apache Camel {} ({}) is starting",
                    env!("CARGO_PKG_VERSION"),
                    self.name
                );
                self.start_components().await?;
                let mut routes: Vec<Arc<DefaultRoute>> = self
                    .routes()
                    .into_iter()
                    .filter(|r| r.is_auto_startup())
                    .collect();
                routes.sort_by_key(|r| r.startup_order().unwrap_or(i32::MAX));
                for (i, r) in routes.iter().enumerate() {
                    if let Err(e) = camel_api::service::Service::start(r.as_ref()).await {
                        for started in routes[..i].iter().rev() {
                            let _ = camel_api::service::Service::stop(started.as_ref()).await;
                        }
                        return Err(e);
                    }
                }
                log::info!(
                    "Apache Camel {} ({}) started",
                    env!("CARGO_PKG_VERSION"),
                    self.name
                );
                Ok(())
            })
            .await
    }

    /// Stops the context: the routes in reverse startup order, then the components.
    ///
    /// # Errors
    ///
    /// Returns the first failure (all the routes and components are stopped anyway).
    pub async fn stop(&self) -> CamelResult<()> {
        self.state
            .stop(|| async {
                log::info!(
                    "Apache Camel {} ({}) is shutting down",
                    env!("CARGO_PKG_VERSION"),
                    self.name
                );
                let mut first_error = None;
                let mut routes = self.routes();
                routes.sort_by_key(|r| r.startup_order().unwrap_or(i32::MAX));
                for r in routes.iter().rev() {
                    if let Err(e) = camel_api::service::Service::stop(r.as_ref()).await {
                        first_error.get_or_insert(e);
                    }
                }
                let components: Vec<SharedComponent> =
                    self.components.read().unwrap().values().cloned().collect();
                for c in components.iter().rev() {
                    if let Err(e) = c.stop().await {
                        first_error.get_or_insert(e);
                    }
                }
                self.started_components.lock().unwrap().clear();
                match first_error {
                    Some(e) => Err(e),
                    None => Ok(()),
                }
            })
            .await
    }

    /// Creates a producer template to send messages to endpoints.
    pub fn create_producer_template(&self) -> ProducerTemplate {
        ProducerTemplate::new(
            self.this
                .upgrade()
                .expect("the DefaultCamelContext is held in an Arc"),
        )
    }
}

impl CamelContext for DefaultCamelContext {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn uuid_generator(&self) -> Arc<dyn UuidGenerator> {
        self.uuid.clone()
    }

    fn is_message_history(&self) -> bool {
        self.message_history.load(Ordering::Relaxed)
    }

    fn endpoint(&self, uri: &str) -> CamelResult<SharedEndpoint> {
        self.resolve_endpoint(uri)
    }

    fn properties_component(&self) -> Option<Arc<dyn PropertiesComponent>> {
        Some(self.properties.clone())
    }

    fn plugin(&self, type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        self.plugins.read().unwrap().get(&type_id).cloned()
    }

    fn registry(&self) -> Option<Arc<dyn Registry>> {
        Some(self.registry.clone())
    }

    fn log_listeners(&self) -> Vec<Arc<dyn LogListener>> {
        self.log_listeners.read().unwrap().clone()
    }

    fn resolve_language(&self, name: &str) -> CamelResult<Arc<dyn Language>> {
        if let Some(l) = self.languages.read().unwrap().get(name) {
            return Ok(l.clone());
        }
        let created = lookup_bean::<Arc<dyn Language>>(self.registry.as_ref(), name)
            .map(|l| (*l).clone())
            .or_else(|| {
                self.language_factories
                    .read()
                    .unwrap()
                    .get(name)
                    .map(|f| f())
            })
            .ok_or_else(|| CamelError::no_such_language(name))?;
        let mut languages = self.languages.write().unwrap();
        Ok(languages.entry(name.to_string()).or_insert(created).clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
